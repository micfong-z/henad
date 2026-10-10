---
date: 2026-10-10
title: "Owned descriptor storage"
description: "Milestone M1 of #49. The parameter, stat, stat entry and action descriptors keep private fields over Cow<'static, str>, so a model read at run time can own its strings. Choice options, error lists, view palettes and model metadata follow, and no model code changed."
icon: material/tag-text-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
state: descriptors private and owned or borrowed, palettes owned by snapshots, ModelMetadata and Structure non-exhaustive, ./check.sh and the GPU tests passing, uncommitted on top of 4f1ea05
baseline_commit: 4f1ea05
delta_state: "uncommitted on `49/m1-owned-descriptors`, 86 changed files and this record"
---

# Owned descriptor storage

> Milestone M1 of #49 (dynamic model loading), decision 8 of its plan, which takes decision 2.2(c) of #48.
> A model read at run time, such as a grid rule from a model file, has to own its parameter ids, stat labels, action ids and palette.
> `ParamDescriptor`, `StatDescriptor`, `StatEntry` and `ActionDescriptor` now keep private fields over `Cow<'static, str>`, read through accessors.
> The helpers and `const fn new` keep their `&'static` signatures, and no model file of henad-models, the tutorial or the template changed outside its tests.
> `ParamKind::Choice` holds a `ChoiceOptions`, the error lists own their strings, views borrow their palettes and snapshots own theirs.
> `ModelMetadata` gained a constructor and an owned palette, and it and `Structure` are `#[non_exhaustive]`.
> The schema hashes, the CLI goldens, the tutorial's parity and the byte-identity sweep tests pass unchanged.

## State before

`49/m1-owned-descriptors` was created from `49-dynamic-loading`, which equals `master` at `4f1ea05` (0.3.0 released).
The four descriptor types had public `&'static str` fields, as #48 decided for 0.3.
`ParamKind::Choice` held `&'static [&'static str]`, `ActionDescriptor` was `Copy`, and five error lists, `Column.label` in `StatColumns` and the app's `form_labels` copied `&'static str` out of descriptors.
`GridView`, `PointView` and `EdgeView` held `&'static` palettes, and the CPU snapshots copied the reference.
`ModelMetadata` and `Structure` were exhaustive, and `ModelMetadata.palette` was `Option<&'static [[u8; 4]]>`.

## What was done

### Descriptors (henad-core)

- **`ParamDescriptor`** holds `id` and `label` as `Cow<'static, str>`, with `kind`, `apply` and `format` beside them, all private.
  `id()`, `label()`, `kind()`, `apply()` and `format()` read them.
  `ParamDescriptor::new(id, label, kind)` takes a `&'static str` or a `String` for either string and returns a live parameter in the plain format, and `on_reload`, `percent` and `is_live` are unchanged.
  The four helpers go through `new` and keep their signatures.
- **`ChoiceOptions`** is the new type in `ParamKind::Choice`.
  A private enum holds either a `&'static [&'static str]` (`const fn from_static`) or a `Vec<String>` (`new`).
  It has `len`, `is_empty`, `get`, `iter` and `position`, `From` impls for both kinds, a `Debug` that prints the names as a list, and a `PartialEq` that compares names, so a borrowed and an owned list of the same names are equal.
  `choice_param` wraps its slice in `from_static`.
- **`StatDescriptor`** keeps `label` as a `Cow` and `color` private.
  `const fn new` builds `Cow::Borrowed`, `owned(String, color)` builds `Cow::Owned`, and `label()` and `color()` read them.
  It gained `PartialEq` and `Eq`, and `entry(value)` returns a `StatEntry` of the series.
- **`StatEntry`** keeps `label`, `value` and `color` private, with `new(label, value, color)`, `label()`, `value()` and `color()`.
  The literals in `helpers.rs`, `view.rs`, the tests and the app's test helpers became `StatEntry::new`.
- **`ActionDescriptor`** keeps `id` and `label` as `Cow`s, with `const fn new`, `owned(String, String)`, `id()` and `label()`.
  It is no longer `Copy`, and the two GPU registrations that copied one out of a trait const now clone it.
- **`stat_entries`** takes `&[StatDescriptor]` and calls `StatDescriptor::entry`, so a runtime state can pass a list it owns.
- **The `known` lists** of `ScheduleError::UnknownAction`, `PlanError::UnknownAction` and `UnknownFixed`, `ValueError::UnknownParam` and `FactorError::UnknownParam`, and `ValueError::UnknownOption.options`, are `Vec<String>`.
  Each is built only on the error path.
  `Vec<String>` prints through `Debug` as `&[&str]` did, so every message, the CLI's included, reads as before.
- **`Column.label`**, private in `StatColumns`, is a `String`, filled once when a run plans its columns.

### Palettes and metadata

- `GridView`, `PointView` and `EdgeView` borrow their palettes for `'a`, the lifetime of the view.
- `GridSnapshot`, `PointSnapshot` and `EdgeSnapshot` hold `palette: Vec<[u8; 4]>`.
  `build_snapshot` takes each palette back from the recycled snapshot and refills it with `refill`, as it does the cells, the lanes and the edge list.
  After the first publish no palette allocates.
- `ModelMetadata` is `#[non_exhaustive]`, with `new(backend, structure)` (no palette, replays exactly) and the builder methods `palette(impl Into<Cow<'static, [[u8; 4]]>>)` and `replays_exactly(bool)`.
  Its fields stay public for reading.
  The five `register_*` functions build it through the constructor.
- `Structure` is `#[non_exhaustive]`.
  `LaneSpec`, `BufferSpec` and `PassSpec` inside it stay `&'static`, as the plan fixes.
  The four matches outside henad-core gained a wildcard arm: `prepended_ids` and `metadata` in the kit's `declared.rs`, `size_for_jobs` in `determinism.rs`, and `structure_rows` in the app's Model panel.
  Clippy's `match_same_arms` then folded the GPU arms of `prepended_ids` and `size_for_jobs` into the wildcard, and a comment names both cases.
  The wildcard in `metadata` fails the `Metadata` check with "declares a structure that the testing kit does not check", so a variant added later fails the kit until the topology comparison gains an arm for it.
- The app's `EdgeLayer` keeps the palette of its last upload beside the version and length, and uploads again when the snapshot's palette differs.
  Before, a palette change with no new graph version never reached the instance buffer.

### Call sites

Every read of a descriptor field became an accessor call.
A script read the `E0616` diagnostics from `cargo check --message-format=json` and appended `()` at each primary span, which covered most of the roughly 250 reads.
A second pass removed the borrows that became needless (`&descriptor.kind()`).
The rest was done by hand: matches on `*descriptor.kind()` where the arms copy numbers, `options.iter()` in `schema_hash`, the `--params` JSON and the CLI's `--params` text, and the `matches!` arms of the history tests.
`form_labels` returns `Vec<(&'a str, f32)>` tied to the schema, and `stat_bits` in the kit returns labels tied to the entries.
Three test helpers that edited a descriptor in place (`OversizedGpuSir`, `params_before_change` in the results store, and `BuggyState`'s zeroed stats) now rebuild the descriptor or the entry.

The facade re-exports `ChoiceOptions` from `henad::params`, and `tests/facade_paths.rs` names it in `named_only`.
The doc example of the facade's `params!` reads `descriptors()[DENSITY].id()`.

### Model code

`git diff` under `crates/henad-models/src`, `examples/tutorial/src` and `templates/model-project` shows no change to model code.
The changes there are all inside test modules or `tests/` directories, where tests read stat values and descriptor ids: the `mod tests` blocks of seven henad-models models, `src/tests/`, three consistency tests, the test modules of the tutorial's `gpu_life` and `gpu_foraging`, and `tests/parity.rs`.
Two guide pages, `gpu-game-of-life.md` and `gpu-ants.md`, carry copies of those two tutorial test modules, and `tests/snippets.rs` holds each to its file, so both pages changed with them.
`tests/parity.rs` changed only in its field reads (`d.id` to `d.id()`, `e.value` to `e.value()`), and one closure that returned borrowed strings from a consumed list now owns them.
Its assertions are unchanged.

### Tests

| Test | Where | What it holds |
|---|---|---|
| `owned_and_borrowed_descriptors_hash_alike` | `henad-core/src/explore/fingerprint.rs` | An owned copy of a schema, a choice included, has the same `schema_hash`, plan hash and results fingerprint as the borrowed one |
| `descriptors_build_in_const_context` | `henad-core/src/params.rs` | `StatDescriptor::new`, `ActionDescriptor::new` and `ChoiceOptions::from_static` run in a `const`, and equal their owned forms |
| `borrowed_and_owned_options_read_alike` | `henad-core/src/params.rs` | `len`, `get`, `position` (first of two equal names), `iter`, `Debug` and equality agree between the two storages |
| `a_choice_with_owned_options_parses_by_name` | `henad-core/src/explore/value.rs` | An owned choice reads a name and an index, writes a name, and reports the owned option and id lists in its errors |
| `a_snapshot_palette_is_refilled_without_reallocating` | `henad-compute/src/cpu/sim_thread.rs` | A palette the state owns reaches the grid and agent layers, an edit reaches the next publish, and all three palettes keep their allocation across a recycle |
| `an_owned_param_descriptor_round_trips_through_a_spec` | `henad-explore/src/spec_file.rs` | A spec that sets virus_network's `network` choice by name plans against owned descriptors as against the compiled ones, writes the name back, and plans the same after a TOML round trip |

The `Composite` test model in `sim_thread.rs` now owns its palette, so the existing snapshot tests run against a borrowed-for-`'a` palette too.

### Hot paths touched

No loop changed shape.
These functions sit on a per-tick, per-sample or per-publish path and changed:

- `stat_entries` (every CPU and GPU model's `stats()`, per sample and per publish): one `Cow` clone per series, a pointer and length copy for a borrowed label, in place of a `&'static str` copy.
  An owned label allocates once per series per sample.
  No compiled model has one.
- `StatDescriptor::entry` (new, behind `stat_entries` and `StatsHistory::entries`).
- `helpers::stat`, `stat_vec2` and `stat_histogram` (models that build entries themselves): `StatEntry::new` with `Into<Cow>`, the same work.
- `build_snapshot` (per publish): three `refill` calls of a few dozen bytes each, into recycled `Vec`s.
- `Column::value` (per sample, sweeps and `--export-stats`): reads `entry.value()`.
- `AgentLayer::widen_colors`, `EdgeLayer` upload, `expand_grid`, `sample_grid` and the export's `grid_pixels` (per frame or per upload in the app): borrow the snapshot's `Vec` where they took a `&'static` slice.
- `EdgeLayer::upload` (per frame in the app) also compares the snapshot's palette with the uploaded one, a few dozen bytes, before it returns early.

`step()`, `run_pass`, the kernels, `ScatterGrid` and the GPU encode paths are untouched.

### Docs

- `docs/authoring/parameters.md`: the `ParamKind` block shows `ChoiceOptions`, and two short paragraphs name the accessors, `ParamDescriptor::new`, `from_static` and `ChoiceOptions::new`.
- `docs/authoring/statistics.md`: `label()`, `color()`, `const fn new` and `owned`.
- `docs/authoring/views.md`: a view borrows its palette, and a publish copies it.
- `docs/authoring/model-sets.md`: `ModelMetadata` and `Structure` are `#[non_exhaustive]`, and a match on the structure takes a wildcard arm.
- `docs/developing/releasing.md`: the stability bullet on descriptors describes the new storage, and `ModelMetadata` and `Structure` join the `#[non_exhaustive]` lists.
- `docs/guide/first-model/gpu-game-of-life.md` and `gpu-ants.md`: the test copies above.
- `CHANGELOG.md`: one Added line and four breaking Changed lines under Unreleased.
- `AGENTS.md`: the henad-core paragraph describes the storage, the wildcard rule for `Structure` and the five places that hold one, and the snapshot paragraph the palette copy.

### Review pass

Two reviews read the change before this record was final.
The fixes they led to: the edge layer's upload on a palette change, the kit's `Metadata` check failing an unknown structure, the `model-sets.md` sentence, the fuller CHANGELOG lines, the AGENTS.md paragraph naming which constructor borrows and which owns, and doc wording in `action.rs`, `view.rs`, `params.rs`, `metadata.rs`, `views.md` and four test docs that described models read at run time as if they existed.
The owned-label allocation stays as the plan's `Cow` has it, recorded below.

### Edited tree

```text
.
├── AGENTS.md                                  ~ descriptor storage, snapshot palettes
├── CHANGELOG.md                               ~ Unreleased: one Added, four breaking Changed
├── crates/
│   ├── henad-core/src/
│   │   ├── params.rs                          ~ private ParamDescriptor, ParamDescriptor::new, ChoiceOptions, tests
│   │   ├── view.rs                            ~ private StatDescriptor and StatEntry, owned, entry, views borrow palettes
│   │   ├── action.rs                          ~ private ActionDescriptor, owned, no Copy, owned known list
│   │   ├── metadata.rs                        ~ ModelMetadata::new and builders, Cow palette, non_exhaustive
│   │   ├── helpers.rs                         ~ helpers through the constructors, signatures unchanged
│   │   ├── export/stats_csv.rs                ~ Column.label String
│   │   └── explore/                           ~ value, factor, plan, design_csv, fingerprint, reducer, stop, genome
│   ├── henad-compute/src/
│   │   ├── snapshot.rs                        ~ owned palettes
│   │   ├── cpu/sim_thread.rs                  ~ palette refill, a_snapshot_palette_is_refilled_without_reallocating
│   │   ├── entry/mod.rs                       ~ ModelMetadata::new, cloned GPU action descriptors
│   │   └── simulation.rs, cpu/network_engine.rs, gpu/*   ~ accessors
│   ├── henad-explore/src/                     ~ accessors, schema JSON, kit wildcard arms, round-trip test
│   ├── henad-cli/src/                         ~ accessors, choice options joined through iter
│   ├── henad-app/src/                         ~ accessors, palettes by reference, form_labels lifetime, Model panel arm,
│   │                                            edge upload on a palette change
│   ├── henad-models/                          ~ tests only
│   └── henad/                                 ~ ChoiceOptions in henad::params, named in facade_paths
├── examples/tutorial/                         ~ tests only (gpu_life, gpu_foraging test modules, tests/parity.rs)
└── docs/
    ├── authoring/parameters.md, statistics.md, views.md, model-sets.md   ~ accessors, owned constructors
    ├── guide/first-model/gpu-game-of-life.md, gpu-ants.md ~ test copies
    └── developing/
        ├── releasing.md                       ~ stability policy
        └── agent-record/
            └── 20261010-50-owned-descriptors.md   +
```

`zensical.toml` gains the nav entry for this record.

## State after

Everything is uncommitted on `49/m1-owned-descriptors`, on top of `4f1ea05`.

All checks ran on the final tree:

- `./check.sh` passes: 1209 tests passed and 4 ignored, six more than record #47 counted, which are the six new tests.
  That covers the wasm32 typechecks, fmt, packaging, cargo-deny, clippy, the doc tests, the README program, the docs with `-D warnings`, the scripts' tests and the web build.
- `HENAD_REQUIRE_GPU=1 cargo test --workspace --all-targets --all-features` passes: 1184 passed, none failed or ignored.
- Run by name and passing: `schema_hashes_are_unchanged_since_0_2_0`, `a_0_2_0_manifest_still_resumes`, `a_0_2_0_manifest_still_replays_and_merges`, henad-cli's `tests/golden.rs` and `tests/json_lines.rs`, the tutorial's `tests/snippets.rs`, `tests/parity.rs` and `tests/shaders.rs`, and `the_readme_program_matches_the_example`.
  The byte-identity sweep tests pass inside the workspace run.
- The template, copied to a scratch directory and patched to the worktree's `henad` and `henad-build` through `[patch.crates-io]`, passes `scripts/ci.sh lint test` with `RUSTFLAGS` unset and `HENAD_REQUIRE_GPU=1`, `every_model_conforms` included.
- Two `./check.sh` runs before the passing one stopped at `fault::tests::a_location_survives_a_panic_on_a_rayon_worker`, with "a worker panic must still carry its location".
  The test flakes at the base commit too: a copy of `4f1ea05` from `git archive` failed it in 4 of 15 runs of `cargo test -p henad-compute --lib --all-features` on this machine, which was busy with parallel builds.
  `fault.rs` is untouched here.
- The scratch directory outside the worktree, which held the template copy, the base copy, two helper scripts and the logs, is deleted.
- The app was not run.
  The egui check that design.md's M1 lists (a Choice parameter, the Sweep builder's labels, chart legends) is left to the round's UI stage, along with the palette swatches of the Model panel and the colours of the grid, agent and edge layers.

Proposed commit: `feat: owned descriptor storage behind accessors`.

### Benchmark gate, for a quiet machine

No timing ran in this session, since parallel builds kept the machine busy.
The plan's gate is interleaved release runs of `henad-cli` on game_of_life, sir, boids and ants, old against new, flat.
`henad-cli` times `step()` alone, and no step changed, so the plain runs check that nothing else moved.
The `--export-stats` runs add a stats sample per tick, which exercises `stat_entries`, `StatEntry::new` and `Column::value`.
The publish path (`build_snapshot`) is measured only by the live runner, which round 4's milestone R gates.

```bash
# Once, from the root of a checkout holding this change: an old tree at the base commit and both release binaries.
NEW_TREE=$(git rev-parse --show-toplevel)
git worktree add /tmp/henad-m1-old 4f1ea05
(cd /tmp/henad-m1-old && cargo build --release -p henad-cli)
(cd "$NEW_TREE" && cargo build --release -p henad-cli)
OLD=/tmp/henad-m1-old/target/release/henad-cli
NEW=$NEW_TREE/target/release/henad-cli

# Interleaved, three rounds, flat-out step() throughput.
for round in 1 2 3; do
  for model in game_of_life sir boids ants; do
    $OLD $model --steps 1000 --reps 3 --json > "old-$model-$round.jsonl"
    $NEW $model --steps 1000 --reps 3 --json > "new-$model-$round.jsonl"
  done
done

# Interleaved, with a stats sample on every tick.
for round in 1 2 3; do
  for model in game_of_life sir boids ants; do
    /usr/bin/time -p $OLD $model --steps 2000 --export-stats "/tmp/old-$model.csv" 2>> "old-stats-$model.txt"
    /usr/bin/time -p $NEW $model --steps 2000 --export-stats "/tmp/new-$model.csv" 2>> "new-stats-$model.txt"
  done
done
# Each pair is expected to match byte for byte. All four are deterministic CPU models.
for model in game_of_life sir boids ants; do cmp "/tmp/old-$model.csv" "/tmp/new-$model.csv"; done

git worktree remove /tmp/henad-m1-old
```

## Issues found & future directions

- **A wildcard arm hides a new `Structure` variant from henad's own crates.**
  `#[non_exhaustive]` forces a wildcard in henad-explore and henad-app, and a variant added later compiles there without a new arm.
  The kit's `Metadata` check fails on such a variant, so a registered model of the new structure fails the registry tests until the kit compares it.
  The other arms, `prepended_ids`, `size_for_jobs`, the app's `structure_rows` and the `matches!` in `benchmark.rs`, pass it silently, and AGENTS.md lists all five.
- **An owned stat label allocates per sample.**
  `stat_entries` clones the label into each `StatEntry`.
  A runtime model with owned labels pays one small allocation per series per sample, in the sweep sampler and in the live runner's stats refresh.
  Compiled models pay nothing.
  The plan fixes `Cow` as the storage, and M1 keeps it.
  M4's gate, the first with owned labels, should look for this cost.
  If it shows, a private label type over `&'static str` or `Arc<str>` behind the same `&str` accessors makes the clone a reference count, with no change to the public API.
- **No mutation API on descriptors.**
  Three tests rebuilt a descriptor to change one default, and a rebuilt descriptor is live and plain.
  None of them needed the apply mode or the format, but a host that edits declarations, as #37's nodes will, may want a `with_kind` or a builder.
- **`StatDescriptor::entry` is public.**
  It is the one-line form of what `stat_entries` does for one series, and a model building its entries by hand can use it.
  It is a new public item beyond the plan's list.
- **`ChoiceOptions::is_empty` is new.**
  Clippy's `len_without_is_empty` asks for it beside `len`.
- **The benchmark gate and the egui check are pending.**
  Neither ran in this session, as State after describes.
  Both belong before the commit.
- **`a_location_survives_a_panic_on_a_rayon_worker` flakes under load,** at the base commit as well, as State after describes.
  A likely cause is the global fallback for the location, which a concurrent test's panic can reach first.
  It deserves its own fix outside #49.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
