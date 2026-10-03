---
date: 2026-10-02
title: "Library M10c: the tutorial crate"
description: "The tenth milestone of #48, third part. The finished code of the five first-model pages moves out of henad-models' tests into examples/tutorial, a workspace member that names every item through the henad facade, carries its own shader copies, and runs the parity tests and the testing kit. The guide and the authoring reference move to facade paths with it."
icon: material/package-variant
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M10c implemented, with `HENAD_REQUIRE_GPU=1 ./check.sh` and the docs build green
baseline_commit: 366a764
delta_state: uncommitted on `48-library`, on top of M10b's commit
---

# Library M10c: the tutorial crate

> M10c is the third part of the tenth of the eleven milestones of #48, which turns Henad into a library published on crates.io.
> The finished code of the five first-model pages moves from `crates/henad-models/src/tests/tutorial/` into `examples/tutorial`, the package `henad-tutorial`, which is a workspace member and never published.
> Its library and its tests name every item through the `henad` facade, and its two GPU models build their own shader copies through henad-build and `henad::include_shaders!`, as a downstream crate does.
> A test holds each shader copy to the shipped one byte for byte, the parity tests move with the models, and a new test runs the testing kit over the five.
> The tutorial needed no item the facade lacked, but the authoring prelude gains ten names that the CPU trait signatures and field layers force on every model page.
> The five first-model pages and the authoring reference move to facade paths in the same change.

## State before

`48-library` stood at 366a764 (M10b, the facade), with a clean tree.
The tutorial twins sat in `crates/henad-models/src/tests/tutorial/` as test modules of henad-models, about 2,100 lines across eight files.
They imported from seven paths across henad-core and henad-compute, and the two GPU twins bound the shipped shaders through henad-models' own `shader_bindings`, under the names `gpu_game_of_life` and `gpu_ants`.
The pages printed `crate::shader_bindings::gpu_life` and `gpu_foraging`, so a twin and its page disagreed there.
The five pages' import snippets used henad-core and henad-compute paths, and the GPU pages' test snippets still called `crate::tests::support::headless_context`, which M10a had replaced with `headless_test_device`.
Record #40 flagged `wrap_index`, `wrap_coord`, `axis_delta`, `offsets`, `for_each_neighbor` and `heading_octant` as left out of the authoring prelude, for M10c to settle.

## What was done

### The crate

`examples/tutorial` holds `Cargo.toml`, `build.rs`, `src/` and `tests/`.

- **Manifest.** `publish = false`, the workspace's version, edition, MSRV, licence and lints.
  The one normal dependency is `henad` by path, with no features.
  The build dependency is `henad-build` from the workspace.
  The dev-dependencies take `henad` again with `example-models` and `testing`, plus `log` and `rayon`.
  [4.1]'s table gives henad-models by path as the dev-dependency.
  Taking the example models through `henad::models` keeps the tests on facade paths, as asked, and adds no edge the normal graph lacks.
  The root `Cargo.toml` names the member, as `members = ["crates/*", "examples/tutorial"]`.
- **Build script.** `stamp_commit()` and `ShaderBuild::discover("src")?.generate()`, the template's two lines from [4.3.3].
  A second `cargo build -v -p henad-tutorial` reports `Fresh henad-tutorial`.
- **Library.** `lib.rs` keeps the old `mod.rs` doc, rewritten for the new home, then `henad::include_shaders!()`, the five modules, and `models()`, which registers the five under the crate's own `build_info!()`, in the shape of the template's `models()` [4.3.4].
  The files are where a page tells a reader to make them: `life.rs`, `foraging/{mod,field}.rs`, `virus.rs`, `gpu_life/mod.rs` and `gpu_foraging/mod.rs`, each moved with `git mv`.
- **Imports.** Each model file opens with `use henad::authoring::prelude::*;`.
  The macros come from the root (`henad::params!`, `henad::actions!`, `henad::buffers!`, `henad::agent_lanes`, `henad::for_each_chunk_mut`), and the GPU agent vocabulary from `henad::authoring` (`BufferSpec`, `Domain`, `PassCtx`, `NUM_AGENTS`, `split_params` and the rest), as [8] asks of a GPU page.
  `GRID_INIT_SEED`, `AGENT_INIT_SEED` and `AgentLanes` come from `henad::authoring` as well.
  The unit tests reach the states through `henad::engine`, `SimState` and `GpuSimState` through `henad::runner`, `wgpu` through `henad::gpu`, `StatEntry` through `henad::stats`, and the test device through `henad::testing`.
  `grep -rn henad_ examples/tutorial` finds the crate's own name, `henad_tutorial`, in its tests, `henad_build` in `build.rs`, and `henad_models` in a comment of the copied `gpu_life/display.wgsl`.
- **Shaders.** `gpu_life/` holds copies of `gpu_game_of_life/{step,display,reduce}.wgsl`, byte for byte.
  `gpu_foraging/` holds `gpu_ants/{state,step,merge,display,reduce}.wgsl` with `gpu_ants::state` read as `gpu_foraging::state`, in the `#define_import_path` of `state.wgsl` and the imports of `step.wgsl` and `reduce.wgsl`.
  The action shaders `clear.wgsl`, `randomise.wgsl` and `reset_colony.wgsl` stay behind, since no page declares an action on a GPU model.
  The twins now bind `crate::shader_bindings::gpu_life` and `gpu_foraging`, the paths the pages print.

### The tests

| File | Tests | Origin |
|---|---|---|
| `src/life.rs` | `a_blinker_rotates_and_comes_back` | Moved |
| `src/foraging/mod.rs` | `results_do_not_depend_on_the_thread_count` | Moved |
| `src/virus.rs` | `the_virus_walks_a_path_and_stops_at_a_resistant_node` | Moved |
| `src/gpu_life/mod.rs` | `the_alive_count_matches_the_cpu_model` | Moved |
| `src/gpu_foraging/mod.rs` | `a_run_replays_bit_identically` | Moved |
| `tests/parity.rs` | The 12 parity tests, `the_game_of_life_tutorial_matches_the_shipped_model` and its siblings | Moved from `src/tests/tutorial/parity.rs` |
| `tests/shaders.rs` | `the_gpu_life_shaders_equal_the_shipped_ones`, `the_gpu_foraging_shaders_equal_the_shipped_ones_apart_from_the_import_path` | New |
| `tests/kit.rs` | `the_tutorial_models_conform` | New |

The parity tests became an integration test.
They read nothing but public items, and an integration test is a second crate on the facade, so the parity test checks the facade from outside as well.
The page-owned tests stay inline, since the pages include their files whole.

`tests/shaders.rs` reads both sides with `include_str!`, the shipped side through `../../../crates/henad-models/src/`.
The `gpu_foraging` comparison replaces `gpu_ants::state` with `gpu_foraging::state` on the shipped side, compares the whole text, and asserts that the shipped shaders name `gpu_ants::state` exactly three times.
A fourth mention then fails the test where a blind replace would hide it.

`tests/kit.rs` is the template's `every_model_conforms` [4.3.4] with a warning when no adapter exists.
Every CPU model skips the four GPU checks and every GPU model skips `ThreadCount`, each as `does not apply to this backend`, and nothing else is skipped.

### The facade

The tutorial needed no item without a facade path, so `facade_paths.rs` is unchanged.
`henad::authoring::prelude` gains ten names, each one a type the pages' CPU models cannot avoid naming:

| Added to the prelude | Where a model page names it |
|---|---|
| `Extent` | Every `AgentModel` and `NetworkModel` signature (`from_params`, `init`, `DEFAULT_EXTENT`) |
| `NoIndex`, `StepCtx` | `type Index` and `run_step_pass` of an agent model |
| `Nodes`, `NodeCtx`, `Network` | `init`, `run_node_pass` and `stats` of a network model |
| `ScalarFieldSpec`, `Combine` | A scalar field layer's trait and its `COMBINE` |
| `Deposits`, `ScalarRead` | `run_deposit_pass` and the read view of a scalar field |

`wrap_index`, `wrap_coord`, `axis_delta`, `offsets`, `for_each_neighbor` and `heading_octant` stay at `henad::authoring::primitives::space`, since no page uses them.
The GPU agent vocabulary stays at `henad::authoring` as [8] has it, and so do the engine's seeds and parameter indices.

### Packaging and the licence page

- The `package` job has excluded `henad-tutorial` since M1, and `cargo package --workspace --exclude henad-tutorial --no-verify --locked` still packages every crate.
- `scripts/check_packaging.sh` reads `crates/*` alone, and a comment now says that examples/tutorial never packages and reads the example models' shaders.
- `about.toml` sets `[private] ignore = true`.
  `docs/license.html`, regenerated with cargo-about 0.9.1, is unchanged.
  Without that table the page would list henad-tutorial, which a run with the table removed confirmed.
- henad-models' package no longer carries the tutorial, which sat under `src/` and shipped in its tarball.

### The docs

- **Whole-file includes.** The six of [7] point at `examples/tutorial/src/` (`life.rs`, `foraging/mod.rs`, `foraging/field.rs`, `virus.rs`, `gpu_life/mod.rs`, `gpu_foraging/mod.rs`), each page's repository sentence names the new path and links to it on GitHub, and none of the six carries a `title=`.
  The eight shader includes on the two GPU pages also point at the tutorial's copies.
  The GPU ants page then shows `gpu_foraging::state`, the import path the page writes, where it showed `gpu_ants::state` under the heading `gpu_foraging/state.wgsl`.
  The closing sentences say the shaders are copies of the shipped port's own.
- **The first-model pages.** Each page's first import is `use henad::authoring::prelude::*;`.
  An import snippet that only added prelude names became a sentence saying the prelude holds them, and one that added names outside it now shows only those (`use henad::for_each_chunk_mut;`, `use henad::authoring::GRID_INIT_SEED;`, the GPU vocabulary from `henad::authoring`, `use henad::authoring::AgentLanes as _;`).
  The macros read `henad::params!`, `henad::actions!` and `henad::buffers!`, and the tests' imports match the files.
  The two GPU test snippets call `headless_test_device(&TestDeviceRequest::baseline())`, and the note under the GPU Life test explains it.
  The GPU ants page's note on its `as _` imports now explains the one left, and its parameter note names `NUM_AGENTS`, `WORLD_WIDTH` and `WORLD_HEIGHT` instead of `cpu::agent_engine`.
- **`reference/primitives.md`.** The example imports read `henad::authoring::primitives::{space, rng}`, the two section lines give the Rust path and the WGSL module apart, `NeighborhoodKind` sits at `henad::authoring::NeighborhoodKind`, and "Related, elsewhere" names `SpatialHash`, `Extent`, `reduce_chunks`, `Network` and `label_components` at `henad::authoring`.
  Two new sentences say which primitives the authoring prelude holds and where the other space helpers come from.
- **Other pages.** `authoring/performance.md` names `henad::authoring::SpatialHash`, `authoring/agent-models.md` says `SpatialHash` and `NoIndex` are at `henad::authoring` and in the prelude, and `authoring/statistics.md` names `henad::authoring::label_components`.
  The sweep for inner paths a reader types also moved `authoring/network-models.md` (`Network`), `authoring/shaders.md` (`henad::authoring::primitives`), `authoring/determinism.md` (`henad::testing` behind the facade's `testing` feature, and `henad::install_panic_hook`), `reference/models.md` (`henad::models::example_models()`) and `reference/cli.md` (`henad::Simulation`).
  Three inner paths are left in place on purpose, listed under issues below.

### AGENTS.md and the changelog

- AGENTS.md: "The workspace has 8 crates, and the tutorial crate beside them", a `henad-tutorial` line in the diagram's legend, the tutorial's dependencies after the dependency rule, a `henad-tutorial` crate bullet after the facade's naming its files, its import rule, its build script, the three test files and its packaging exclusions, and the foraging tutorial in the list of `results_do_not_depend_on_the_thread_count` tests.
- CHANGELOG: two Added lines (the prelude's ten names, `examples/tutorial`) and one Changed line (the guide and the reference on `henad::` paths, the includes from `examples/tutorial`).

### Edited tree

```text
.
├── AGENTS.md                               ~ the tutorial crate, its tests, the crate count
├── CHANGELOG.md                            ~ the prelude, examples/tutorial, the docs on facade paths
├── Cargo.toml                              ~ examples/tutorial as a member
├── Cargo.lock                              ~ the henad-tutorial package
├── about.toml                              ~ [private] ignore = true
├── zensical.toml                           ~ nav entry #41
├── scripts/check_packaging.sh              ~ a comment on what the crate list leaves out
├── crates/henad/src/lib.rs                 ~ ten names in henad::authoring::prelude
├── crates/henad-models/src/tests/
│   ├── mod.rs                              ~ no tutorial module
│   └── tutorial/                           - moved to examples/tutorial
├── examples/tutorial/                      + the package henad-tutorial
│   ├── Cargo.toml
│   ├── build.rs                            + stamp_commit and discover("src")
│   ├── src/
│   │   ├── lib.rs                          ← tests/tutorial/mod.rs, + include_shaders!, models()
│   │   ├── life.rs                         ← tests/tutorial/life.rs
│   │   ├── foraging/{mod,field}.rs         ← tests/tutorial/foraging/
│   │   ├── virus.rs                        ← tests/tutorial/virus.rs
│   │   ├── gpu_life/mod.rs                 ← tests/tutorial/gpu_life.rs
│   │   ├── gpu_life/{step,display,reduce}.wgsl            + copies
│   │   ├── gpu_foraging/mod.rs             ← tests/tutorial/gpu_foraging.rs
│   │   └── gpu_foraging/{state,step,merge,display,reduce}.wgsl   + copies
│   └── tests/
│       ├── parity.rs                       ← tests/tutorial/parity.rs
│       ├── shaders.rs                      + the copies against the shipped shaders
│       └── kit.rs                          + the kit over models()
└── docs/
    ├── guide/first-model/*.md              ~ facade imports, includes from examples/tutorial
    ├── reference/{primitives,models,cli}.md                ~ facade paths
    ├── authoring/{performance,agent-models,statistics,network-models,shaders,determinism}.md   ~ facade paths
    └── developing/agent-record/20261002-41-library-tutorial.md   +
```

## State after

Everything is uncommitted on `48-library`, on top of 366a764, in the main checkout as asked.
Nothing is staged.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 1067 tests, none failed, with the wasm32 typechecks, packaging, cargo-deny, the docs and the web build.
  M10b ended at 1064, and this session adds the two shader tests and the kit test. The 17 moved tests run in henad-tutorial in place of henad-models.
- `uv run --locked zensical build` passes with no issues, and the rendered pages carry the `examples/tutorial` listings.
- `HENAD_REQUIRE_GPU=1 cargo test -p henad-tutorial` runs 20 tests, all passing: the five inline, the kit, the 12 parity tests and the two shader tests.
- A second `cargo build -v -p henad-tutorial` reports `Fresh henad-tutorial`.
- `cargo clippy -p henad-tutorial -p henad -p henad-models --all-targets --all-features -- -D warnings -W clippy::all` passes.
- `cargo package --workspace --exclude henad-tutorial --no-verify --locked` (with `--allow-dirty` for the uncommitted tree) packages every crate.
- `cargo +1.95 check --workspace --locked`, the `msrv` job, passes with henad-tutorial in the workspace.

Proposed commit: `feat: tutorial crate`.

## Issues found & future directions

- **Three inner paths stay in the docs.** `gpu-game-of-life.md:260` shows `henad_models::game_of_life::PALETTE` in a WGSL comment the page copies from the shipped shader, and the copy has to stay byte for byte. `developing/architecture.md:52` describes where the planning code lives. `authoring/network-models.md:282` names `henad_compute::cpu::layout`, which a model never imports and which has no facade path.
- **`chunk_seed` has no facade path.** `reference/primitives.md` lists it among things a kernel reaches for, with its file. No model calls it today, since `run_pass` and the engines seed each chunk. A model that draws inside `for_each_chunk_mut!` would need it, and adding it to `henad::authoring` then is one line.
- **The pages still describe henad-models.** Their snippet titles read `crates/henad-models/src/...`, registration goes into `example_models()`, the run lines name `-p henad-cli` and `--bin henad-app`, and "Registering the model also opted us into the registry tests" points at henad-models' tests. The GPU Life page includes `crates/henad-models/build.rs:shader_build`. [8] rewrites all of it for the template in M11, and the finished files now match that template's shape already.
- **The prelude's glob shadows nothing today**, but a page that defines a type named like a prelude name (`Network`, `Nodes`) shadows the prelude's silently. A glob import loses to a local item, so this cannot break a build, only confuse a reader.
- **The CI `package` job's manifest filter** watches `crates/*/Cargo.toml` and the root files, not `examples/tutorial/Cargo.toml`. The job excludes the tutorial anyway, so nothing is missed.
- **The kit test leans on the device.** Without an adapter and without `HENAD_REQUIRE_GPU`, the kit skips the GPU checks with a warning, as the example models' registry test does.
- **Next.** M10d builds the template on the facade, and its `vote` model follows the tutorial's import shape.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
