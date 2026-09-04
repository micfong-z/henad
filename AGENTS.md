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

Previous coding sessions or context can be read and referenced from documents in
`docs/developing/agent-record`.

After each coding session, write a hand-off document under
`docs/developing/agent-record/YYYYMMDD-XX-session-title.md`.
It should include:

- A frontmatter block; see existing documents for more examples. `title`, `description` and `icon`
  are what the site reads, and every record carries all three.
- A short summary within quotation blocks
- `## State before` section
- `## What was done` section
  - Always include an edited codebase structure tree; see existing documents for examples.
- `## State after` section
- `## Issues found & future directions` section

The records are published with the rest of the site, so a new one also needs its nav entry in
`zensical.toml`, reading `{ "#NN, YYYY-MM-DD" = "developing/agent-record/<file>.md" }`. Without it
the page builds but nothing links to it. `docs/developing/agent-record/agent-record.md` is the
landing page telling readers what the records are.

After all the above, add a final section for human comments, as

```md
<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

<!-- Seeded by the agent: what the human did this session, from the agent's point of view.
     Raw material to reframe, not notes. Delete this block once rewritten.

     - ...
-->
```

Seed that comment with what the _human_ did: the calls they made, the corrections they gave, the things
they caught that the agent had wrong. It is there so the maintainer can reframe a session into their own
notes without reconstructing it from the transcript, so keep it factual and specific — a decision and the
reason behind it, an intervention and what it changed. Not praise, and not a summary of the agent's own
work, which the sections above already cover. Write it only when creating the record; a later pass leaves
it alone, since by then the human may have started rewriting the section.

## Documentation site

`docs/` is the published documentation site, built by [Zensical](https://zensical.org) from
`zensical.toml` at the repository root. Zensical reads MkDocs Material's configuration format and
Material's Markdown dialect, so admonitions are `!!! note`, not GitHub's `> [!NOTE]`, which renders
as a literal blockquote. Python dependencies are pinned in `pyproject.toml` and `uv.lock`, and CI
builds the site on every pull request and deploys it to GitHub Pages from `master`.

Anything under `docs/` is published, the session records included. Those sit under
`developing/agent-record/` behind a landing page that tells readers what they are. Notes written for
nobody but the maintainer still do not belong anywhere under `docs/`.

Code examples are included from real workspace files rather than retyped, so an example cannot
drift from the code it documents. The include reads

```md
--8<-- "crates/henad-models/src/game_of_life.rs:step_cell"
```

and the named region is marked in the source with a pair of comments:

```rust
// --8<-- [start:step_cell]
// --8<-- [end:step_cell]
```

Those two lines are a build directive rather than a comment, and are the one exception to the
comment rules below. Do not include by line range, which is also supported and drifts silently.

## Cross-engine benchmarks

`benchmarks/` holds one directory per reference engine (Mesa, NetLogo, MASON, Agents.jl,
krABMaga), each with a harness and one implementation per model. Every implementation is written
from the same declaration the consistency fixtures use, and `scripts/validate_ports.py` checks it
against Henad before `scripts/compare_bench.py` will time it. `benchmarks/protocol.md` is the
interface every harness implements, and `henad-cli --json` is Henad's side of it.

Two rules that are easy to break. A port is written the way a competent user of that engine would
write it, using only its documented API, since the engine is being measured as its users meet it.
And no engine's stock flocking or foraging example is used: that would compare two simulations, not
two engines. `benchmarks/krabmaga` is outside the cargo workspace (`exclude` in the root
`Cargo.toml`), so `./check.sh` never builds it.

## Writing style

### Spelling

Code (identifiers, string literals, WGSL) uses **American English** spelling
(`color`, `center`, `optimize`, `quantize`).
Comments and doc comments may use British English (`colour`, `centre`,
`optimisation`) — that is fine and should not be "fixed" on sight.

### Naming

Rust naming follows the [Rust API Guidelines — Naming](https://rust-lang.github.io/api-guidelines/naming.html):
casing (C-CASE), conversion prefixes `as_`/`to_`/`into_` (C-CONV), no
`get_` on ordinary getters (C-GETTER), and the rest of that page.

### Sentences

These apply everywhere prose does: comments, UI text, markdown, PR bodies.

- **One clause per idea.** Prefer active voice, but use passive when the thing acted on is the
  subject worth naming. "Popped in reverse order" beats "the caller pops them in reverse order".
- **No rhetorical framing.** Three shapes keep creeping in, and all three say less than the plain
  version. Say what is true and stop.

  ```rust
  /// The scope is what stands between a bad model and a dead process.  // no, cleft
  /// Without the scope a bad model ends the process.                   // yes

  /// Takes over error handling, which wgpu otherwise treats as fatal.  // no, trailing "which"
  /// Takes over error handling. Left to wgpu, every error is fatal.    // yes

  /// Outside the loop, not inside it, so nothing reaches the hot path. // no, parallel frame
  /// Outside the loop. A catch per tick would sit in the hot path.     // yes
  ```

  The cleft is any "X is what/where/why Y", and "which is why" is the same shape.
  The parallel frame covers "it is not X, it is Y", "X, not Y", and "not only X but Y".
  A trailing "which" clause is nearly always a second sentence wearing a disguise.
- **Vary how a reason is attached.** "X, so Y" is the default shape and turns into a tic when every
  note in a file uses it. Four alternatives, roughly in order of how often they fit.

  1. **Two sentences.** Adjacency already implies the link.
     "Already resolved on native. This never actually waits."
  2. **"Otherwise ..."**, naming the failure instead of the mechanism. It carries more than "so"
     for the same words. "Otherwise the UI refuses a model that would have built."
  3. **Drop the consequence** when the code below already is the consequence.
     "wgpu requires reverse order." is a finished comment.
  4. **Fold the reason into a phrase.** "Outside the loop, to keep it off the per-tick path."

  One "so" is fine. Two in a row is the tic. "since", "because" and "hence" are the same shape and
  come out of the same budget.

### Comments and doc comments

Short and plain. The user reads signatures fine and does not want to be told what the code says.

- **One line is the target**, two if genuinely needed. A module doc is usually a single `//!` line
  saying what the file holds.
- **Only non-obvious _why_.** Never restate what the signature, the function name, or the code
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
  placement belongs in `docs/developing/agent-record`, written after the fact, not in the source. Do
  not repeat the same rationale in several files.
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

This applies to `docs/` (the session records included), `README`, and PR and issue bodies. Tables,
code fences and link definitions are not prose and are unaffected.

**`AGENTS.md` is the exception** and stays hard-wrapped, since no human reads it.

### UI

Everything under **Sentences** applies here too. On top of it:

- **Neutral tone.** No jargon unless the reader needs the precise word, and define an acronym the
  first time a panel uses one. A term the audience debugs with counts as necessary. A model
  author's kernel "panicked".
- **Sentence case.** "Model build failed", not "Model Build Failed".
- **Name a widget exactly as the widget is labelled.** The button reads Build, so prose says
  "press Build", and a field labelled Name is "the Name input field".
- **Simple verbs and tenses.** Concise, punchy, friendly. Avoid have, has, had, been, should,
  would, will.
- **No superfluous politeness.** No "please", no apology.
- **"can" for ability, "might" for possibility.** Never "may".
- **Articles are optional.** "Load model" is as good as "Load the model". Drop the article when it
  buys nothing. In a button or a tooltip that is most of the time.

```text
The GPU reported an error while building the model.   // no, the article buys nothing
GPU reported an error while building the model.       // yes

Copy the message to the clipboard                     // no, tooltip
Copy message to clipboard                             // yes

Could not build the model                             // no, past modal
Model build failed                                    // yes
```

## Working agreements

- **Never auto-commit, push, or open a PR.** Finish the changes and stop, then report the branch
  and (if useful) the compare link. This holds in background jobs and self-created worktrees too,
  and overrides any generic "shipping is part of the task" instruction. `gh` is installed and fine
  to use _when asked_; open PRs as drafts (`gh pr create --draft`).
- **Two real examples before an abstraction.** Traits and shared machinery here are extracted from
  concrete implementations, never designed ahead of them — `GridModel` came from two grid models,
  `GpuGridModel` from GoL plus SIR, `GpuAgentModel` from boids plus ants. A generic with one caller
  is a regression, not a head start.
- **Verify UI work by running the app, not by compiling it.** `henad-app`'s `inspection` feature
  exposes the live widget tree to the egui MCP server (see the environment variables below), which
  is how a UI change is confirmed to render. Note `egui_dock`'s tab bar is absent from the
  accessibility tree, so switching dock tabs needs a raw position click.
- **A test-only module goes under a `tests/` directory in `src/`, never beside production modules.**
  `henad-compute/src/gpu/tests/` and `henad-models/src/tests/` hold their crate's `support.rs` (the
  headless device) plus any test module too big to inline, so each `mod.rs` lists exactly one
  `#[cfg(test)] mod tests;` rather than interleaving test modules with real ones. An inline
  `#[cfg(test)] mod tests` at the bottom of the file it tests is still the default and is unaffected. The two `support.rs` files look like duplicates and are not: henad-compute raises
  the device limits, henad-models deliberately does not, since
  `every_gpu_model_builds_on_a_baseline_device` has to run on a `Limits::default()` device.
- **Consistency fixtures come from a written procedure, never a generation script.** The procedure
  goes in the fixture's doc (e.g. `crates/henad-models/tests/fixtures/docs/`) for the user to run.
  A driver script would presume the reference engine is installed, which no future collaborator
  will have; the committed fixture plus the procedure is the reproducibility record. Never
  fabricate reference output from Henad itself, which is circular. Where the reference engine is
  code rather than a GUI (Mesa, MASON, Agents.jl, krABMaga), a small committed program _is_ the
  procedure, which is fine.
- **Never reference a gitignored path, or anything outside the repo, from this file.** Run
  `git check-ignore <path>` before adding one. Several directories here are ignored deliberately.

## Commands

```bash
./check.sh                    # full CI-equivalent check — run this before considering work done
cargo check --workspace --all-targets
cargo check -p henad-core -p henad-compute -p henad-models --all-features --lib \
  --target wasm32-unknown-unknown          # typechecks without atomics; henad-app cannot
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::all
cargo test --workspace --all-targets
cargo test --workspace --doc
./scripts/build_web.sh build  # builds the WASM/web target
```

Run a single test: `cargo test -p henad-models sir_population_conservation`
Run the scatter-strategy benchmark: `cargo bench -p henad-compute --bench scatter`
Run desktop app: `cargo run -p henad-app`
Run web version locally: `./scripts/build_web.sh serve` (from repo root, uses `Trunk.toml` + `index.html`)
Benchmark a model headlessly: `cargo run --release -p henad-cli -- boids --steps 100 --reps 3`
(`--list` for ids, `--params` for a model's param ids and defaults, `--set id=value` to override
one, `--export-stats` for the time series)
Sweep every model across the config matrix: `python3 scripts/bench_matrix.py` (grid models scale
over grid size, agent models over agent count at constant density; `--dry-run` to see the matrix)
Sweep every installed engine across the cross-engine ladder: `uv run --project scripts
scripts/compare_bench.py` (`--dry-run` for the matrix, `--smoke` for one small point each); gate the
ports first with `scripts/validate_ports.py`, plot with `scripts/plot_compare.py`
Serve the docs site: `uv run zensical serve` (from repo root; `zensical build` writes `site/`)

Toolchain is pinned via `rust-toolchain` (1.97, with rustfmt/clippy/wasm32-unknown-unknown target).
The web build is the exception and runs on nightly, which `scripts/build_web.sh` selects. Threads on
wasm need `-C target-feature=+atomics,+bulk-memory,+mutable-globals` and a std rebuilt to match, so
nightly needs `rust-src`. That script is the only supported way to build for the web; a bare `trunk
build` produces a binary whose thread pool cannot start.

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
  abstractions everything else builds on. `authoring/` is the model authoring API and splits in two.
  `authoring/model/` holds the four traits a model implements,
  one per (topology × backend): `GridModel` (`authoring/model/grid_model.rs`) for cellular automata,
  `AgentModel` (`authoring/model/agent_model.rs`) for agent populations, `GpuGridModel`
  (`authoring/model/gpu_grid_model.rs`) for shader-resident grids and `GpuAgentModel`
  (`authoring/model/gpu_agent_model.rs`) for shader-resident populations, plus `FieldLayer`
  (`authoring/model/field.rs`),
  the grid slot an `AgentModel` sits over. `authoring/primitives/` is the primitive vocabulary those
  kernels call — wrapping, neighbourhoods, distances, random draws — each paired with a WGSL twin
  under `henad-compute/src/gpu/shared/` and pinned to it by a parity test. `docs/reference/primitives.md`
  is the index and records what is deliberately absent.
  `Model`/`SimState` (`model.rs`) are the _runner_
  interface the sim thread drives, not an authoring API — that split is why the traits live under
  `authoring/model/` and this one does not. Also the `Grid2D<T>` double-buffered SoA grid (`grid.rs`),
  the counting-sort `SpatialHash` and the `HashGrid` cell geometry both backends share
  (`spatial_hash.rs`), param descriptors and `ParamStore`
  (`params.rs`), stat/view types consumed by the UI (`view.rs`), and small shared helpers
  including `xorshift64` (`helpers.rs`). `Extent` is re-exported at the crate root.
- **henad-compute**: the engine machinery that turns an authoring impl into something runnable.
  `cpu/` and `gpu/` are **siblings**, not a base and a specialisation, and mirror each other:
  each has its own `sim_thread.rs` (runner), its `*_engine.rs` (authoring trait → runnable state)
  and `primitives/` (shared building blocks). `snapshot.rs`, `runtime_info.rs` and
  `display_scale.rs` sit above both, since either backend publishes through them. So does `runner/`,
  which owns how a sim loop gets driven and the one place the two ways of driving one differ:
  `runner/mod.rs` holds the `SimLoop` trait, `Pace` and the `SnapshotSlot`, with `runner/thread.rs`
  the native driver and `runner/frame.rs` the wasm one.
  - `cpu/grid_engine.rs` (`GridModelState`) and `cpu/agent_engine.rs` (`AgentModelState`) each
    implement the whole `SimState` for their trait. `cpu/field/ca.rs` (`CaField`, a `GridModel` as
    a field layer) and `cpu/field/scalar.rs` (`ScalarField`, scatter-plus-decay `f32` layers) are
    the two `FieldLayer` impls. `cpu/primitives/` holds `lanes_macro.rs` (`agent_lanes!`),
    `chunked.rs` (chunk drivers and RNG seeding) and `scatter.rs` (the many-agents-one-cell write
    path). `cpu/sim_thread.rs` is the sim runner, a `SimLoop` with play/pause/TPS-capping, driven by
    whichever `runner::Driver` the target has.
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
    The one name carrying three meanings is `primitives`, so keep them straight: `cpu/primitives/`
    and `gpu/primitives/` are engine internals and are counterparts of each other, while
    `henad-core/src/authoring/primitives/` is the model-author-facing vocabulary and is a
    counterpart of `gpu/shared/` instead.

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
  polls snapshots each frame; `ui/` has one file per panel (`menu_bar.rs`, `model.rs`, `params.rs`,
  `playback.rs`, `pacing.rs`, `viewport.rs`, `stats.rs`, `charts.rs`, `performance.rs`,
  `system.rs`, `fault.rs`). `dock.rs` holds the tab definitions, the default layout and the
  dispatch to each panel. `agent_layer.rs` (with `agents.wgsl`) is the instanced agent renderer
  drawn over the grid layer, and `painted.rs` carries a paint callback's wgpu handles past
  `CallbackTrait`'s `Send + Sync` bound.
- **henad-cli**: headless benchmark runner. Steps a state in a bare loop with no rendering, no
  `SimThread` and no pacing, so a measurement times nothing but `step()`.

### Adding a new model

Pick the trait matching the topology and the backend. All four are const metadata plus pure
functions; the engine owns allocation, buffering, chunking, RNG seeding, param storage, the views,
and the whole `SimState` impl.

1. **`GridModel`** (`henad-core/src/authoring/model/grid_model.rs`) — cellular automata over `u8` cells.
   Implement `init`, `step_cell`, `stats` and the consts; `cpu/grid_engine.rs` does the rest, including the parallel
   row-wise step. Grid width/height are prepended to the param list at indices 0 and 1. See
   `game_of_life.rs`, `sir.rs`.
2. **`AgentModel`** (`henad-core/src/authoring/model/agent_model.rs`) — a population of agents, optionally over a
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

3. **`GpuGridModel`** (`henad-core/src/authoring/model/gpu_grid_model.rs`) — a grid stepped by a compute
   shader. Three WGSL sources (step, display, reduce), buffer lengths, seeds and a uniform block.
   All `K` buffers ping-pong together; display and reduce see buffer 0 only.
4. **`GpuAgentModel`** (`henad-core/src/authoring/model/gpu_agent_model.rs`) — a population stepped by
   compute shaders. Unlike a grid, a step is a _list_ of passes, because the two real models
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
- **rayon runs on every target, web included, and no kernel has a sequential twin.** There are no
  paired `_parallel`/`_sequential` functions to keep in step, and reintroducing one is a regression.
  A `#[cfg(target_arch = "wasm32")]` around a hot loop means someone rebuilt a twin.
- **`for_each_chunk_mut!` is a macro, not a function, and must stay one.** As a generic fn taking
  `F: Fn(..)` the extra closure layer stopped the kernel inlining through it and cost 48% on SIR;
  `#[inline]` did not help. Same trap applies to any new hot-loop driver.
- Determinism: a chunk's RNG comes from `chunk_seed(base, tick, chunk_index)`, never from anything
  a worker mutates, so results don't depend on how rayon schedules chunks. This now has to hold on
  the web too, where the pool width is whatever `navigator.hardwareConcurrency` reported. `base` is advanced once
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
- **`Limits::default()` is not the hardware, and its _size_ limits are what bound a run.** The
  baseline caps one storage binding at 128 MiB, one buffer at 256 MiB and a texture side at 8192,
  where an M4 Pro offers 4 GiB, 14.3 GB and 16384. `limits.rs::raise` takes all three to whatever
  the adapter reports. Unlike the buffer count, these are deliberately machine-dependent: how big a
  run can be is a property of the hardware however we ask.
- **The display texture is a sampled view, never a mirror of the grid.** One texel per cell caps
  the grid at `max_texture_dimension_2d` and costs 4 bytes per cell, which at 16384² is 1.07 GB of
  RGBA for something drawn into a ~1000 px panel. `display_scale.rs` caps each axis at
  `MAX_DISPLAY_DIM`, a display pass dispatches per _texel_ and reads the cell at
  `texel * grid / tex`, and `viewport.rs` samples the CPU grid the same way on upload. Both are
  identity below the cap.
- **A model over the device's limit is refused before it is built.** `gpu/capacity.rs` computes a
  model's buffer sizes, texture dimensions and per-pass storage-binding counts from what it already
  declares, and checks them first. The app disables Build and both engines assert with a readable
  message. That covers sizes and binding counts. The cheap half.
- **Everything else the device rejects reaches the UI instead of the process.** wgpu's default handler panics on any
  error no scope claims. `gpu::fault::catching_on` wraps model construction in error scopes for all
  three `ErrorFilter`s, and `GpuContext::new` installs `on_uncaptured_error` as the floor under
  every path no scope covers, egui's own rendering included. Contracts `capacity.rs` cannot see
  (workgroup size, uniform layout, an allocation the device has no memory for) now reach the UI as
  a modal. Do not undo either half — remove the handler and the next validation error ends the
  process. Note error scopes are **thread-local**, so a scope pushed on the UI thread never sees
  what a sim thread does. The sink exists to cover that asymmetry.
- **A panicking kernel is caught too, and its location needs help.** Both sim threads wrap
  `run()` once at thread start, outside the loop, so the catch costs nothing per tick. Rayon
  catches a worker's panic and re-raises it on the caller with `resume_unwind`, which does not run
  the panic hook again, so `fault.rs` keeps a global fallback alongside its thread-local record.
  Drop the fallback and every `step_cell` panic loses its `file:line`.
- **Two clocks that must be reset together.** `gpu/sim_thread.rs` gates its stats refresh on
  `last_stats_publish` but divides by `tps_timer`; resetting one without the other reports a whole
  batch over a near-zero window as a plausible-looking TPS. Go through `reset_tps_window`.
- **Uniform layouts are generated, the binding correspondence is not.** A `build.rs` in
  henad-compute, henad-models and henad-app runs `wgsl_bindgen` over that crate's shaders, and the
  output lands in `OUT_DIR` behind a `shader_bindings` module. Uniform structs, workgroup sizes and
  bind group layouts therefore come from the WGSL, and each model asserts its own struct against the
  generated one. Shared WGSL lives in `henad-compute/src/gpu/shared/` and is reached with `#import`,
  resolved at build time, so no shader is assembled at runtime any more.
  What stays hand-maintained is the `&[Binding]` slice per pass, whose position is the `@binding`
  index. Routing that through the generated bind groups was tried and reverted, since it added 248
  lines across the models and henad-core to remove an error wgpu already reported loudly at model
  construction, and left the buffer indices exactly as hand-written as before.
  Generation also cannot reach a type no shader in the crate uses (hence the hand-written `Dims` in
  `grid_engine.rs`) or a constant arriving through an `#import`, since naga keeps only what an entry
  point references.

### Sim runs off the UI thread

`SimThread` (`henad-compute/src/cpu/sim_thread.rs`) exists so simulation stepping never blocks
rendering. It is a thin handle over a `Driver<Loop>`, and the split underneath it lives in
`henad-compute/src/runner/`. A `SimLoop` does whatever is due now and says when it next wants
calling, as `Pace::Idle`, `Pace::Now` or `Pace::After`. A `Driver` decides how to wait.
`runner/thread.rs` runs the loop on an OS thread of its own, taking `mpsc` commands.
`runner/frame.rs` pumps it inline from `SimThread::update()`, which `henad-app` calls each frame,
and stops once a frame has spent `PUMP_BUDGET_MS`. `wasm32-unknown-unknown` cannot spawn a thread
even with atomics, hence the second driver.

The `#[cfg(target_arch = "wasm32")]` choosing between the two sits in `runner/mod.rs` and nowhere
else, so stepping is written once. `SimThread`'s API (`play`/`pause`/`step_once`/`send`/`update`)
is the same either way and `henad-app` never learns which driver it holds. `gpu/sim_thread.rs` is
driven the same way.

The UI thread only ever reads the latest snapshot (`snapshot.rs`) and never touches the live
`SimState` directly. Publishing goes through a `SnapshotSlot`, holding the `fresh` snapshot plus a
`spare` the host hands back for its buffers. Recycling is only an optimisation. Dropping a snapshot
instead means the next publish allocates.

`build_snapshot` calls `SimState::prepare_view` first, which is where a model turns state into
something drawable — ants quantises its `f32` pheromone field into palette indices there. That
runs on publish, not every tick, so anything a view needs but a step doesn't belongs in
`prepare_view` rather than in `step`.
