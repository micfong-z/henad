---
date: 2026-10-10
title: "GPU grid engine from a spec"
description: "Milestone M3 of #49. The GPU grid engine is non-generic and builds from a private spec that holds a model's declarations as values, with GpuGridState<M> as a thin adapter that reads the spec from its model type. The public API of henad-compute is unchanged, and the GPU sweep, track, parity and kit tests pass as they were."
icon: material/expansion-card-variant
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#49"
state: "spec, engine and adapter built and reviewed, public API compared with the base, ./check.sh and check_hot.sh passing with HENAD_REQUIRE_GPU=1 after the review fixes, uncommitted on top of 2f62d9c"
baseline_commit: 2f62d9c
delta_state: "uncommitted on `49/m3-gpu-grid-spec`, one new source file, one new test file, this record, and seven changed files"
---

# GPU grid engine from a spec

> Milestone M3 of #49 (dynamic model loading), in round 2 of its plan.
> A `GpuGridModel` declares itself through associated consts and functions without `self`, and only a type can carry those, so a model read at run time could not reach the GPU grid engine.
> The engine now builds from `GpuGridSpec`, a crate-private struct that holds the declarations as values: the shader sources and binding lists as `Cow`s, the stat series, and the model's functions behind a private `GridHooks` trait.
> The non-generic `GpuGridEngine` owns every buffer, pipeline and bind group and implements `SimState` and `GpuSimState`, and `GpuGridState<M>` reads the spec from `M` and forwards every call.
> The engine computes the workgroup counts of each pass and the bytes of its buffers at construction, and a step calls no hook.
> A rustdoc JSON comparison with the base finds every public item, signature, trait impl, auto trait and doc of henad-compute unchanged, apart from the parameter names of one forwarder.
> Five new tests pin the spec's reading of a model, three on a probe model in henad-compute and two on `gpu_game_of_life` and `gpu_sir` in henad-models.
> Two reviews followed, and the session fixed a stale AGENTS.md sentence on where the engines monomorphise and three doc comments that narrated the design.

## State before

`49/m3-gpu-grid-spec` branched from `49-dynamic-loading` at `2f62d9c`, which holds the round-1 milestones M1 (owned descriptors, record #50) and D1 (the dylib core) as placeholder commits.
`dev-docs/49-dynamic-loading/plan.md` governs #49 and puts M3 in round 2 beside M2 and D2, each in a worktree of its own.
The plan's grid rule template (M4) builds a runtime GPU grid model on this engine: owned WGSL text, a fixed list of bindings, and functions that read the rule's own data.

`GpuGridState<M>` read its model through `M::` at every step of its life.
`new_seeded` read the consts and called `M::dims`, `buffer_lens`, `seed_buffers`, `step_params_bytes` and `action_params_bytes`.
`encode_steps` and `encode_action` called `M::step_dims` per encode, `stats()` called `M::stats`, and `heap_bytes()` called `M::buffer_lens`, which allocates a `Vec`, on every publish.
`demand` and `max_storage_bindings` read the consts through a private `declared_passes`.
The whole engine was monomorphised once per model, in the crate that registers it.

## What was done

### The spec (`gpu/grid_spec.rs`, new)

`GpuGridSpec` holds everything `GpuGridState<M>` read from `M`:

| Field | Type | From the trait |
|---|---|---|
| `id` | `Cow<'static, str>` | `ID` |
| `workgroup_size` | `u32` | `WORKGROUP_SIZE` |
| `buffers` | `&'static [&'static str]` | `BUFFERS` |
| `step`, `display`, `reduce` | `GridShaderSpec` (`source: Cow<'static, str>`, `bindings: Cow<'static, [BindingDecl]>`) | the three shaders and their binding lists |
| `actions` | `Vec<GridActionSpec>` (`id: Cow<'static, str>`, `shader: GridShaderSpec`) | `ACTIONS` |
| `stats` | `Cow<'static, [StatDescriptor]>` | `STATS` |
| `hooks` | `Box<dyn GridHooks>` | `dims`, `buffer_lens`, `step_dims`, `seed_buffers`, `step_params_bytes`, `action_params_bytes`, `stats` |

`GpuGridSpec::from_model::<M>()` borrows every const, and `GridHooks` for the private `ModelHooks<M>` calls each associated function of the same name.
The hooks are one boxed trait object, not seven `Arc<dyn Fn>`s: the model's hooks are a zero-sized type, and boxing one allocates nothing.
The fields are `pub(crate)`, so a crate-private struct literal is the constructor M4's rule uses, with `Cow::Owned` text and whichever binding list it builds.

The spec also carries what the adapter and the engine compute from the declarations:

- `dims(params)`, the hook's size with each side clamped to at least 1.
- `demand(params, limits)` and `max_storage_bindings()`, through the private `declared_passes`, moved here unchanged.
- `assert_contracts()`, the buffer label and workgroup size asserts the engine made inline, in the same order and with the same messages.

`contracts.rs` and `capacity.rs` are unchanged.
Their helpers already take a model id, label text, shader text and binding slices, and the spec now feeds them.

### The engine and the adapter (`gpu/grid_engine.rs`)

`GpuGridEngine` is the old `GpuGridState<M>` body with `M::` replaced by the spec.
It keeps an `Arc<GpuGridSpec>` and every wgpu object it kept before, and adds four cached fields: `step_workgroups`, `cell_workgroups`, `texel_workgroups` and `buffer_bytes`.
It implements `SimState`, `GpuSimState` and a `Debug` that prints the model id, the tick and the grid size.

`GpuGridState<M>` holds the engine and a `PhantomData<M>`.
`new`, `new_seeded`, `demand` and `max_storage_bindings` keep their signatures, and each builds a spec from `M`.
Its `SimState` and `GpuSimState` impls forward every method the old impls defined, and its `Debug` prints what it printed before.
The five method docs the old trait impls carried (`step`, `act`, `set_param`, `encode_steps`, `view`) sit on the forwarders, where the public documentation shows them.

`entry/mod.rs` is unchanged.
`register_gpu_grid_model` still calls `GpuGridState::<M>::max_storage_bindings`, `new_seeded` and `demand`, and boxes a `GpuGridState<M>`.
M2 rewrites that function over `ModelEntry::new_gpu` in the same round, and leaving it alone keeps the merge to M2's side.

### Calls on the per-encode, per-sample and per-publish paths

No allocation and no dynamic dispatch was added inside `encode_steps`.
These calls changed:

- `encode_steps` (per batch): reads the cached `step_workgroups` in place of `M::step_dims` and two `div_ceil`s.
  The vtable call into `GpuGridState<M>` now forwards through one direct call into the engine.
- `encode_action` (per press): `action_params_bytes` is one virtual call through `GridHooks`, and allocates its `Vec<u8>` as before.
- `encode_snapshot_passes` and `encode_stats_passes` (per sample): read the cached texel and cell workgroups.
- `stats()` (per sample and per publish): the `stats` hook is one virtual call, and `stat_entries` reads the `Cow` through a deref.
- `heap_bytes()` (per publish on the GPU sim thread): returns the cached byte count, and no longer allocates the `buffer_lens` vector.
- `GpuGridState::demand` and `max_storage_bindings`: build a spec per call.
  The boxed hook of a model allocates nothing, and the actions vector allocates once when the model declares actions.
  The app's Model panel calls `demand` once per frame, and the call already formats a label per buffer and per pass.

### Public API check

The public surface of henad-compute was compared through rustdoc JSON, built for the base (an archive of `2f62d9c`) and for this tree with the pinned nightly, `--all-features` and `--document-hidden-items`.
A script printed one normalised line per public item, field, variant, impl and impl member, with every id replaced by the path it names, and the two lists were diffed, with and without the docs.

- Every item, signature, trait impl, blanket impl and auto trait impl is the same, `GpuGridState`'s `Send`, `Sync`, `Unpin`, `UnwindSafe` and `RefUnwindSafe` among them.
- Every doc is the same.
- One difference remains: `GpuGridState`'s `set_param` forwarder names its parameters `index` and `value`, where the old impl named them `_index` and `_value`.
  No caller can name a parameter of a trait method.

`GpuGridSpec`, `GridShaderSpec`, `GridActionSpec`, `GridHooks` and `GpuGridEngine` are `pub(crate)`, and `grid_spec` is a private module of `gpu`, next to `contracts`.

### Tests

The existing tests pass unchanged: `interleaved_gpu_runs_match_sequential_ones`, `a_gpu_schedule_matches_export_stats`, `a_pipelined_gpu_sample_matches_a_blocking_one`, the kit's `BaselineBuild` and `FullSubmission` (through `the_example_models_conform`), `gpu/tests/parity.rs`, the tutorial's `tests/parity.rs`, `tests/shaders.rs` and `tests/kit.rs`, the registry tests and the GPU contract tests.

| Test | Where | What it holds |
|---|---|---|
| `a_model_spec_borrows_its_declarations_and_calls_its_functions` | `henad-compute/src/gpu/grid_spec.rs` | The spec of a probe model borrows its id, sources, binding lists and stats, holds its buffers, workgroup size and action, and each hook returns what the function of the same name returns |
| `a_spec_demand_names_each_buffer_and_pass` | same | The demand names both sides of each buffer and each pass, an action included, counts storage bindings per pass, and clamps a zero side to 1 |
| `an_action_shader_of_another_workgroup_size_is_refused` | same | `assert_contracts` checks an action shader held as owned text |
| `the_demand_of_each_gpu_grid_model_follows_its_declarations` | `henad-models/src/tests/gpu_grid_adapter.rs` | For `gpu_game_of_life` and `gpu_sir`, `GpuGridState::demand` names each buffer side at the length the model returns, the display texture, and every pass with the storage buffers its declarations bind. `max_storage_bindings` equals the widest |
| `a_built_gpu_grid_state_follows_its_model_declarations` | same | On a baseline device, a built state of each model reports the model's stat labels and colours in order, the values of `stats` over zeroed counters before a readback, the cell count, the bytes of its buffers and texture, its id in `Debug`, and runs each declared action and no other |

The second henad-models test needs a device and skips without one, and fails without one under `HENAD_REQUIRE_GPU`.
A mutation check confirmed the first one is not vacuous: reading the display shader's bindings from the reduce list made it fail on `gpu_game_of_life_display`.
The spec is `pub(crate)`, and henad-models reaches it only through `GpuGridState`'s public functions.
A test in henad-compute against the two example models would need a dev-dependency from henad-compute on henad-models, a new edge.

### Docs

- `docs/developing/gpu-backend.md`: `grid_spec.rs` in the tree, and a paragraph on the spec, the engine, the adapter and the cached counts.
- `AGENTS.md`: the henad-compute GPU paragraph names the spec, the engine and the adapter, inserted as whole lines after the `Dims` sentence.
  The `entry/` sentence on where the engines monomorphise names the GPU grid engine as the one henad-compute compiles once.
- `CHANGELOG.md`: one Changed line under Unreleased, not breaking.

### Review findings

Two reviews read the first version of this milestone and found no blocker. The session fixed:

- AGENTS.md's claim that every engine monomorphises in the crate that calls `register_*`, stale now that the GPU grid engine is compiled once in henad-compute;
- the module doc of `grid_spec.rs`, which ended its first sentence on a trailing "which" clause and gave the reason the spec exists;
- the `GpuGridSpec` doc's sentence on reading every declaration from a spec and never from a model type;
- the `GpuGridEngine` doc's sentence on where the trait methods are documented.

One finding needed no change in this milestone: `GpuGridState::demand` builds a spec per call, and the actions vector of a model with actions allocates once per call of the app's Model panel.
The entry rewrite listed under the issues below removes it.

### Edited tree

```text
.
├── AGENTS.md                                       ~ grid spec, engine and adapter, where engines monomorphise
├── CHANGELOG.md                                    ~ Unreleased: one Changed line
├── crates/
│   ├── henad-compute/src/gpu/
│   │   ├── mod.rs                                  ~ mod grid_spec
│   │   ├── grid_spec.rs                            + GpuGridSpec, GridShaderSpec, GridActionSpec, GridHooks,
│   │   │                                             ModelHooks, demand, contracts, three tests
│   │   └── grid_engine.rs                          ~ GpuGridEngine from a spec, GpuGridState<M> as an adapter
│   └── henad-models/src/tests/
│       ├── mod.rs                                  ~ mod gpu_grid_adapter
│       └── gpu_grid_adapter.rs                     + the two example GPU grid models through the adapter
├── docs/developing/
│   ├── gpu-backend.md                              ~ the spec, the engine and the adapter
│   └── agent-record/
│       └── 20261010-52-gpu-grid-spec.md            +
└── zensical.toml                                   ~ nav entry #52
```

## State after

Everything is uncommitted on `49/m3-gpu-grid-spec`, on top of `2f62d9c`.

All checks ran with `HENAD_REQUIRE_GPU=1` on this Mac (M4 Pro).

- `./check.sh` passes, three times: twice before the review fixes, and once on the final tree after them.
  Each run counted 1235 tests passed, 6 ignored and none failed.
  That covers the wasm32 typechecks, fmt, packaging, cargo-deny, clippy, the doc tests, the README program, the docs with `-D warnings`, the scripts' tests and the web build.
- `./scripts/check_hot.sh` passes, before the review fixes and on the final tree: `hot-check` with every check `ok`, `gpu_game_of_life` and `gpu_sir` among the twelve models the plugin steps like the static copy, then `hot-check relink`.
- Run by name before the full runs and passing: the three henad-explore GPU tests above, every test of henad-models' library (the registry, contract, runner and model tests), henad-compute's parity test, and the tutorial's `tests/parity.rs`, `tests/shaders.rs` and `tests/kit.rs`.
- `a_location_survives_a_panic_on_a_rayon_worker` passed in all three `check.sh` runs.
- The review fixes change doc comments and Markdown alone, and the public API comparison above still holds.
- `uv run zensical build` builds the site with no issues, this record linked from the navigation.
  The `site/` and `.venv/` it left in the worktree, both gitignored, are deleted.
- The scratch directories outside the worktree held the archive of the base commit, the rustdoc JSON target, the comparison script, a backup used by the mutation check, and the logs of every run, and are deleted.

The app was not run.
The change touches no UI code, and the GPU runner tests in henad-models (`gpu_thread_publishes_gpu_snapshots_and_runs`, `gpu_thread_teardown_and_respawn_is_clean` and their `gpu_sir` siblings) drive the boxed state through `GpuSimThread` as the app does.

Proposed commit: `feat: GPU grid engine built from a spec`.

### Benchmark gate, for a quiet machine

No timing ran in this session, since other milestones were building in parallel.
The plan's gate is interleaved release runs on `gpu_game_of_life` and `gpu_sir`, old against new, flat.
`henad-cli` times its GPU steps through `stepping.rs`, which calls `encode_steps` once per submission of at most 64 steps.
A 64 by 64 grid makes the encode's CPU cost show, and the larger grids the dispatch.
The `--export-stats` runs take a blocking stats sample on every tick, which calls `encode_stats_passes` and `stats()`, and the CSVs of the two builds are expected to match byte for byte.
The last block runs a small GPU sweep on both builds, and its `series.csv` and `summary.csv` are expected to match byte for byte.

```bash
# Once, from the root of a checkout holding this change: an old tree at the base commit and both release binaries.
NEW_TREE=$(git rev-parse --show-toplevel)
git worktree add /tmp/henad-m3-old 2f62d9c
(cd /tmp/henad-m3-old && cargo build --release -p henad-cli)
(cd "$NEW_TREE" && cargo build --release -p henad-cli)
OLD=/tmp/henad-m3-old/target/release/henad-cli
NEW=$NEW_TREE/target/release/henad-cli

# Interleaved, three rounds, flat-out GPU steps at three grid sizes.
for round in 1 2 3; do
  for model in gpu_game_of_life gpu_sir; do
    for size in 64 1024 4096; do
      $OLD $model --steps 2000 --reps 5 --set grid_width=$size --set grid_height=$size --json \
        > "old-$model-$size-$round.jsonl"
      $NEW $model --steps 2000 --reps 5 --set grid_width=$size --set grid_height=$size --json \
        > "new-$model-$size-$round.jsonl"
    done
  done
done

# Interleaved, with a blocking stats sample on every tick.
for round in 1 2 3; do
  for model in gpu_game_of_life gpu_sir; do
    /usr/bin/time -p $OLD $model --steps 2000 --export-stats "/tmp/old-$model.csv" 2>> "old-stats-$model.txt"
    /usr/bin/time -p $NEW $model --steps 2000 --export-stats "/tmp/new-$model.csv" 2>> "new-stats-$model.txt"
  done
done
for model in gpu_game_of_life gpu_sir; do cmp "/tmp/old-$model.csv" "/tmp/new-$model.csv"; done

# A GPU sweep on both builds, compared apart from the timing columns of runs.csv.
for build in old new; do
  bin=$OLD; [ "$build" = new ] && bin=$NEW
  rm -rf "/tmp/m3-sweep-$build"
  $bin gpu_sir --vary infection_rate=0.1:0.5:0.1 --reps 3 --steps 500 --out "/tmp/m3-sweep-$build"
done
cmp /tmp/m3-sweep-old/series.csv /tmp/m3-sweep-new/series.csv
cmp /tmp/m3-sweep-old/summary.csv /tmp/m3-sweep-new/summary.csv

git worktree remove /tmp/henad-m3-old
```

## Issues found & future directions

- **`replays_exactly` is not in the spec.**
  The milestone's brief listed it, but neither the engine nor anything else in M3 reads it, and an unread field fails `dead_code` under `-D warnings`.
  The entry reads `M::REPLAYS_EXACTLY` for its metadata as before.
  M4's rule fills M2's `EntryDeclarations`, which carry `replays_exactly`, so the rule needs no copy in the spec either.
- **Buffer labels stay `&'static`.**
  `BindingDecl.name` and `buffer_target` return `&'static str`, and `Structure::GpuGrid` holds `&'static [&'static str]`.
  The plan's rule template binds fixed labels (`state`, `tick`), so a `Cow` would only add a case nobody builds.
- **The hooks are a trait with one implementation.**
  M4's rule is the second, and the plan names those two callers.
  The trait has no default methods.
  A rule implements all seven, trivially where it has no actions or packing.
- **The boxed state of a compiled model is the adapter, not the engine.**
  `register_gpu_grid_model` boxes `GpuGridState<M>`, and each runner call passes one forwarder.
  An entry that built the engine from one shared `Arc<GpuGridSpec>` would drop that call, build the spec once instead of per `demand`, and box the same type a rule boxes.
  That rewrite belongs with M2's `ModelEntry::new_gpu`, after the merge.
- **The spec is built per call of `GpuGridState::demand`.**
  The app calls it per frame through `ModelEntry::demand`, at the cost of one small vector for a model with actions.
  The same entry rewrite removes it.
- **The GPU agent engine has the same const-driven shape.**
  It stays generic.
  Nothing reads an agent model at run time in #49, and the plan defers runtime agent models to #37.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
