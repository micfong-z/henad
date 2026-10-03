---
date: 2026-10-02
title: "Library M10b: the facade"
description: "The tenth milestone of #48, second part. The henad crate puts the engine, sweeps and model authoring under one module tree with two preludes, the example models, app, command line and testing kit behind features, and a complete program kept as an example and in its README. The nightly atomics clippy joins CI after its existing findings were fixed."
icon: material/package-variant
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M10b implemented in two handovers, the nightly lint fixes and the facade, with `HENAD_REQUIRE_GPU=1 ./check.sh` and the docs build green
baseline_commit: dada65c
delta_state: uncommitted on `48-library`, on top of M10a's commit
---

# Library M10b: the facade

> M10b is the second part of the tenth of the eleven milestones of #48, which turns Henad into a library published on crates.io.
> The new `henad` crate re-exports the engine, the sweeps and the model authoring API under one module tree, with `henad::prelude` for a program and `henad::authoring::prelude` for a model's source.
> Its four features, `example-models`, `app`, `cli` and `testing`, only add modules, and the two hosts come in with their default features off.
> `crates/henad/examples/complete.rs` is the program of [4.2.7], copied byte for byte into the facade's README, which the crate docs include and a doc test compiles.
> `tests/facade_paths.rs` compiles against facade paths alone.
> CI gains a `features` job, the facade's doc test, its two no-atomics wasm32 lines, and clippy on the pinned nightly with atomics, whose 33 existing findings were fixed first as a separate handover.

## State before

`48-library` stood at dada65c (M10a, the testing kit), with a clean tree.
The workspace held seven crates, and a program depended on henad-core, henad-compute and henad-explore separately, naming items by their crate paths.
Record #38 listed 12 findings of the nightly atomics clippy on henad-app, and records #31 and #39 noted `drop_non_drop` in henad-compute and henad-explore.
No CI job ran that clippy.
`docs/license.html` lacked henad-build, which M5 added, so the lint job's licence check would have failed on this branch.
`docs/developing/releasing.md` does not exist yet, and [5.5] holds the release checklist.

## What was done

### Handover 1: the nightly atomics clippy findings

The lint of [4.10], `RUSTFLAGS="-C target-feature=+atomics,+bulk-memory" cargo clippy --target wasm32-unknown-unknown` on the pinned nightly, stopped at henad-compute, so record #38's 12 findings in henad-app were only the last layer.
It found 33 in four crates (14 in henad-compute, 2 in henad-explore, 5 in henad-models and 12 in henad-app), every one fixed or expected before the facade.

| Crate | Finding | Fix |
|---|---|---|
| henad-compute | `manual_midpoint` (2, `cpu/layout.rs`) | `f32::midpoint` |
| henad-compute | tuple to array (`gpu/grid_engine.rs`) | `tex.into()` |
| henad-compute | `drop_non_drop` (`gpu/sim_thread.rs`, wasm `track`) | The parameter takes `_submission` |
| henad-compute | `arc_with_non_send_sync` (6 in `entry/mod.rs`, `runner/mod.rs` twice, `gpu/view/display.rs`, `gpu/agent_engine.rs`) | `expect` with its reason, under `cfg_attr(all(target_arch = "wasm32", target_feature = "atomics"), ..)` |
| henad-explore | `drop_non_drop` (`output/mod.rs`, `File` has no `Drop` on wasm32) | The file lives in a block that ends before the rename |
| henad-explore | unnecessary `get` before `/` (`exec/mod.rs`) | `workers / lanes`, dividing by the `NonZeroUsize` |
| henad-models | `manual_midpoint` (`boids/mod.rs` twice, `gpu_boids/mod.rs`, `virus_network/wiring.rs`) | `f32::midpoint` and `f64::midpoint` |
| henad-models | tuple to array (`gpu_ants/mod.rs`) | `geom.display.into()` |
| henad-app | `arc_with_non_send_sync` (`state.rs`, `ui/agent_layer.rs` three times) | `expect` with its reason, as above |
| henad-app | `manual_midpoint` (`ui/charts.rs`), tuple to array (`ui/edge_layer.rs`), unnecessary `get` (`ui/sweep/builder.rs`), `as_chunks_mut` (`ui/export/image.rs`) | The suggested form |
| henad-app | `needless_pass_by_ref_mut`, `unused_self` and `let_underscore_untyped` in `ui/sweep/mod.rs`'s cfg-split `ColumnsBuild::poll` and `open_folder_results` | Both functions and their calls are native only. A browser never has a running build or an output folder |

The `arc_with_non_send_sync` findings cannot be fixed in place.
wgpu's handles are `Send` on wasm32 only through `fragile-send-sync-non-atomic-wasm`, which turns itself off under atomics, as `henad_core::send_sync` documents, and the `Arc`s are part of public types (`SharedSlot`, `WakeFn`, `ModelEntry`'s parts).
The `expect`s fire only in the threaded web build, so a native or a no-atomics build never sees an unfulfilled expectation.

The midpoint rewrites in henad-models sit outside every kernel: boids' initial and randomised speed, `gpu_boids`' action uniform, and Virus on a Network's wiring bisection.
`f32::midpoint` computes in `f64` and rounds once, which equals `0.5 * (a + b)` in `f32` for finite inputs that do not overflow, and `f64::midpoint` is `(a + b) / 2` below `f64::MAX / 2`.
The consistency fixtures, the golden CLI output and every thread-count test still pass, so no result moved.

### Handover 2: the facade

`crates/henad/` holds `Cargo.toml`, `README.md`, the licence copies, `src/lib.rs`, `examples/complete.rs` and `tests/facade_paths.rs`.
It has no build script.

- **Manifest.** Normal dependencies on henad-core, henad-compute, henad-explore and bytemuck.
  henad-models and henad-app are optional, and henad-cli is optional under `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`.
  The root `[workspace.dependencies]` gains henad-cli and henad-app with `default-features = false`, so `henad/app` and `henad/cli` compile no example model.
  `testing` turns on `henad-explore/testing`.
  The example carries `required-features = ["example-models", "app"]`, and the docs.rs metadata is [5.6]'s.
- **Module tree.** `src/lib.rs` defines nothing but re-exports, in the modules of [4.1]'s table: the root, `params`, `stats`, `views`, `action`, `gpu`, `runner`, `engine`, `explore`, `benchmark` (native), `authoring` with `primitives` and `prelude`, and the gated `models`, `app`, `cli` and `testing`.
  `henad::authoring` globs the seven `authoring::model` modules and `helpers` flat under `#![deny(ambiguous_glob_reexports)]`, and no two of them share a name today.
  `henad::testing` re-exports the kit module whole, and `henad::models` re-exports the ten modules with their model types.
  Native-only items (`run_spec`, `plan_spec`, `read_directory_series`, `DirectorySeries`, `acquire_headless`, `DeviceError`, `benchmark`, `run_native`, `results_folder`, `AppError`) carry the same gate, and every gated item takes `cfg_attr(docsrs, doc(cfg(..)))`.
- **Root macros.** `pub use henad_core::params` imports the macro and henad-core's `params` module together, which collides with the facade's own `henad::params`.
  The seven macros come in through a glob of a private `root_macros` module instead, and the facade's `params` module shadows the glob's module while the macro reaches the root.
- **Preludes.** `henad::prelude` and `henad::authoring::prelude` hold [4.1]'s lists.
  The authoring prelude takes `NoField` as one of "the field layers", and the three neighbourhood offset tables `MOORE_ROW_MAJOR`, `MOORE_COLUMN_MAJOR` and `VON_NEUMANN`.
- **The complete program.** `examples/complete.rs` is [4.2.7]'s program with three changes the workspace's lints asked for.
  The Cargo.toml header is `//` comments, since `//!` lines tripped `doc_link_with_quotes` on the feature list.
  `#![expect(clippy::print_stdout, reason = ..)]` follows the CLI's crate-level precedent.
  `run_sampled` returns a `#[must_use]` `ControlFlow`, which [4.2.7] dropped, so the callback breaks once nobody is infected and the program reports the tick.
- **README.** The crate's README opens with a feature table, then holds the program in a `rust,no_run` fence.
  The crate docs include it with `app` and `example-models` both on, and read "Henad, a parallel agent-based modelling engine." otherwise.
  `the_readme_program_matches_the_example`, an inline test in `lib.rs`, compares the first `rust,no_run` fence with the example file byte for byte.
- **`tests/facade_paths.rs`.** Every path starts with `henad::`.
  Five tests run without a model: a `GpuAgentAction` from a `PassSpec` with an inline shader and empty bindings, a `SweepSpec` from `henad::explore::spec::{BlockSpec, RunSettings, MeasureSettings, SeedSettings, ActionSpec}` with two `FactorSpec`s and a `SweepOptions` with `Concurrency` and `Shard` set, a match on `SetupError::Param(ValueError)`, a match on `DeviceError`, and `SpatialHash`, `HashGrid` and `label_components` with its `ComponentStats` over a four-node network.
  The module `with_an_entry` compiles the parts that need an entry and never runs: `run_spec` with those options, a paced host that builds a `FaultSink`, hands `SimThread::new` a state with an optional `WakeFn` and reads the `Snapshot` of `take_snapshot`, and a function reading a `RunRow`'s status as a `RunStatus`.
  It carries `#[expect(dead_code)]` with its reason.

### CI, check.sh and the checklist

- `check.sh` runs the facade's two wasm32 lines, `cargo check -p henad --lib` and `--features example-models,testing --lib`, after the existing one, and `cargo test -p henad --doc --features example-models,app` after the workspace's doc tests.
- `.github/workflows/ci.yml`: the `lint` job adds the facade's wasm32 lines, the pinned nightly's install and "clippy with atomics" over `-p henad-app` and `-p henad --features app,example-models`, in `target/clippy-atomics`.
  The `test` job adds the facade's doc test.
  A new `features` job, on pull requests, runs `cargo check -p henad` with no features and then once per feature.
- `docs/license.html` is regenerated with cargo-about 0.9.1, adding henad and henad-build.
- [5.5]'s release checklist, step 2, now gives the facade's verified pass as a command, `CARGO_TARGET_DIR="$(mktemp -d)" cargo package --workspace --exclude henad-tutorial --locked --features henad/example-models,henad/app,henad/cli,henad/testing`, and says both passes take a fresh target directory.
  The design sits under the gitignored `dev-docs/`, so that edit is not part of the handover.

### AGENTS.md and the changelog

- AGENTS.md: "The workspace has 8 crates", the facade in the diagram and its legend, the facade's feature edges in the dependency rule, a `henad` crate bullet after henad-cli's, and the three new lines in Commands with the CI-only list naming the `features` job and the atomics clippy.
- CHANGELOG: two Added lines, the facade with its features and preludes, and the complete example.
  The lint fixes change no public item and get no line.

### Edited tree

```text
.
├── AGENTS.md                               ~ 8 crates, the facade's bullet, Commands
├── CHANGELOG.md                            ~ the facade, the complete example
├── Cargo.toml                              ~ henad-cli and henad-app as workspace dependencies
├── Cargo.lock                              ~ the henad package
├── check.sh                                ~ the facade's wasm32 lines and doc test
├── zensical.toml                           ~ nav entry #40
├── .github/workflows/ci.yml                ~ lint: facade wasm32, atomics clippy; test: facade doc test; features job
├── crates/henad/                           + the facade
│   ├── Cargo.toml
│   ├── README.md                           + the program in a rust,no_run fence
│   ├── LICENSE-MIT, LICENSE-APACHE
│   ├── src/lib.rs                          + the module tree, the preludes, the README test
│   ├── examples/complete.rs                + the program of [4.2.7]
│   └── tests/facade_paths.rs               + the gate
├── crates/henad-compute/src/               (handover 1)
│   ├── cpu/layout.rs                       ~ midpoint
│   ├── entry/mod.rs                        ~ expect under atomics
│   ├── gpu/{agent_engine,view/display}.rs  ~ expect under atomics
│   ├── gpu/grid_engine.rs                  ~ tuple into array
│   ├── gpu/sim_thread.rs                   ~ no drop of a non-Drop value
│   └── runner/mod.rs                       ~ expect under atomics
├── crates/henad-explore/src/               (handover 1)
│   ├── exec/mod.rs                         ~ division by the NonZeroUsize
│   └── output/mod.rs                       ~ the manifest file closes at the end of a block
├── crates/henad-models/src/                (handover 1)
│   ├── boids/mod.rs, gpu_boids/mod.rs      ~ midpoint
│   ├── gpu_ants/mod.rs                     ~ tuple into array
│   └── virus_network/wiring.rs             ~ midpoint
├── crates/henad-app/src/                   (handover 1)
│   ├── state.rs, ui/agent_layer.rs         ~ expect under atomics
│   ├── ui/{charts,edge_layer}.rs           ~ midpoint, tuple into array
│   ├── ui/export/image.rs                  ~ as_chunks_mut
│   └── ui/sweep/{mod,builder}.rs           ~ native-only poll and open_folder_results, division
└── docs/
    ├── license.html                        ~ henad and henad-build
    └── developing/agent-record/20261002-40-library-facade.md   +
```

## State after

Both handovers are uncommitted on `48-library`, on top of dada65c, in the main checkout as asked. Nothing is staged.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 1064 tests, none failed, with the wasm32 typechecks the facade's two included, packaging, cargo-deny, the docs, the facade's doc test and the web build. M10a ended at 1057, and the facade adds its five `facade_paths` tests, the README test and the README doc test.
- `uv run --locked zensical build` passes with no issues.
- The nightly atomics clippy passes on `-p henad-app` and on `-p henad --features app,example-models`.
- `cargo check -p henad` passes with no features and with each feature alone, and both wasm32 lines pass.
- `cargo test -p henad --doc --features example-models,app` compiles the README program.
- `RUSTDOCFLAGS="--cfg docsrs -D warnings" cargo doc -p henad --no-deps --features example-models,cli,app,testing` passes on the pinned nightly, the docs.rs build of [5.6].
- `cargo tree -p henad -e normal -i henad-models` finds no henad-models with no features, with `app`, with `cli`, and with `app,cli,testing`.
- `cargo package -p henad --no-verify --list` lists `Cargo.toml`, `Cargo.lock`, `README.md`, both licences, `src/lib.rs`, `examples/complete.rs` and `tests/facade_paths.rs`, besides the generated `.cargo_vcs_info.json` and `Cargo.toml.orig`.
- The checklist's verified pass with the facade's features ran from an empty target directory, with `--allow-dirty` for the uncommitted tree. It verified all eight crates, the facade compiled with henad-cli and henad-models from the tarballs, in 1 min 31 s.
- The complete example ran end to end in a debug build: the build, the live edit and the action, a 12-run sweep with 12 runs ok, the readback, the replay of run 5, and the app opening on it, whose Playback read "Sweep run 5: config 1, replicate 1" through the egui MCP server.
  The window opened on the second display from a seeded `SIR study/app.ron`, and the folder and the sweep's `sir-rates` folder were removed afterwards.

Proposed commits, in order:

1. `fix: nightly atomics clippy findings`, the 20 files under `crates/henad-{compute,explore,models,app}/src` listed in the tree as handover 1.
2. `feat: facade crate`, everything else.

## Issues found & future directions

- **The program's second half shows little at [4.2.7]'s values.** SIR at infection rate 0.3 on 256 by 256 has burnt out by tick 300, so the second outbreak finds no susceptible cell and `run_sampled` breaks at its first sample, printing "the outbreak ended by tick 300". The program is correct. A smaller first `run_to`, or the outbreak before the epidemic ends, would show the live edit. `guide/library.md` in M11 is the place to retune it, in both copies at once.
- **The README program carries `#![expect(clippy::print_stdout)]`.** Henad's workspace lints deny printing. A reader who copies the program into a crate that runs clippy without that lint gets an unfulfilled-expectation warning.
- **No region markers in `complete.rs`.** [8] includes the example in `guide/library.md` by region, and a marker line would also land in the README copy. The README test then has to skip `--8<--` lines, or the guide includes the whole file. M11 decides.
- **`henad::authoring::prelude` reads [4.1]'s lists literally.** It takes the random draws, `Boundary`, `cell_index`, `offset_cell`, `dist_sq`, the three offset tables, `NoField`, `CaField` and `ScalarField`, and leaves `wrap_index`, `wrap_coord`, `axis_delta`, `offsets`, `for_each_neighbor` and `heading_octant` at `henad::authoring::primitives::space`. M10c's tutorial crate will show whether a model page misses any of them.
- **The facade's reach is checked by hand.** [4.1] promises a facade path for every Henad type a listed item's signature or public field reaches. With the rustdoc gate cut, `facade_paths.rs` covers M10b's list, and types such as `henad_explore::exec::RunRequest` (Q5) stay without a facade path on purpose. A type a signature reaches and the tree misses would show up as a crate path in M10c's tutorial or M10d's template.
- **The removed `from_iter_instead_of_collect` lint.** The pinned nightly warns that the workspace configures a lint clippy removed. It is a warning, `-D warnings` leaves it alone, and stable 1.97 still knows the lint.
- **`docs/license.html` was stale before this session**, without henad-build. The page is regenerated here, and the lint job's check had not run on `48-library` since M5.
- **Next.** M10c moves the tutorial into `examples/tutorial` on facade paths, and M10d builds the template on the facade. Both depend on M10b alone.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
