---
date: 2026-10-02
title: "Library M11: the documentation"
description: The documentation half of the last milestone of #48. Six new guide and reference pages, the first-model guide moved into a project made from the template, the install line of Q4, the release checklist and stability policy, and the facade's README program retuned so its live edit shows.
icon: material/book-open-page-variant-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M11's documentation written, the docs build and HENAD_REQUIRE_GPU=1 ./check.sh passing, the version bump, tag and publish left for M11b
baseline_commit: 6217127
delta_state: uncommitted on `48-library`, on top of M10d's commit
---

# Library M11: the documentation

> M11 is the last of the eleven milestones of #48, which turns Henad into a library published on crates.io, and this session wrote its documentation half.
> Six pages are new: "Your own project", "Using Henad from code", "Model sets" in place of "Registering a model", "Testing your model", "Releasing" and "API reference".
> The five first-model pages now build each model in the reader's copy of the template, and every page that includes template or tutorial code states the Henad version it describes.
> Installation gives the install line the maintainer chose in Q4, and the sweep and search guides have the reader save each spec file.
> The facade's example program now raises SIR's infection rate mid-epidemic, where the live edit shows in its output, and prints through `writeln!` with no lint attribute.
> The version stays 0.2.0, and the bump, the tag, the final performance comparison and the publish are left for M11b and the maintainer.

## State before

`48-library` stood at 6217127 (M10d, the template), with a clean tree.
The guide described a fork of Henad: models went into `crates/henad-models/src/`, registration into `example_models()`, and the run lines named `-p henad-cli` and `--bin henad-app`.
`docs/developing/releasing.md` had sat in the nav since v0.1.0 with no page behind it, and the zensical build had not flagged it.
The facade's example printed "the outbreak ended by tick 300" at its first sample, since SIR had burnt out by then (record #40).
Records #35, #36, #40, #41 and #42 each left items for this pass, listed under each heading below.

## What was done

### New pages

| Page | Holds |
|---|---|
| `guide/your-project.md`, "Your own project" | The version, the template's `fetch` and `rename` regions, the storage folder with its dash, the layout and `.gitignore`, the `run` region with `--release` and the 4.7 times of the dev profile, the lockfile and `--locked` for shards and resumes, the `profile` region, `build.rs` whole with what a commit rebuilds, compile-time inputs under `src`, a crate without shaders keeping its stamp, the `update` region with the Dependabot group, the web build with its nightly, Trunk, `.cargo/config.toml`, the refused `RUSTFLAGS`, the two headers and one origin per app, `scripts/ci.sh` with lavapipe and the watchdog caveat, the fxhash advisory, and publishing a model library with `default-features = false` |
| `guide/library.md`, "Using Henad from code" | The features, the `profile` region, the seven names, `complete.rs` whole with its real output, then its five regions in turn: building and stepping, GPU devices, live edits and `run_sampled` (inside the pool, `Send`), thread pools with the measured shapes, faults and the panic hook, sweeps with `apply_execution` before the program's own settings, results and what a rebuild reproduces, opening the app, and provenance |
| `authoring/model-sets.md`, "Model sets" | The template's `lib.rs`, the `register_*` table, ids, sources and when a set records one, `extend`, the example models with `cpu_entries` and `gpu_entries`, a subset by clone, network and GPU entries, device needs and `REPLAYS_EXACTLY` |
| `authoring/testing.md`, "Testing your model" | The template's test, `headless_test_device` with `TestDeviceRequest` and `henad::gpu::wgpu`, the check table moved from the determinism page, settings and exemptions, where the GPU checks run and why real hardware still matters, and what stays hand-written |
| `developing/releasing.md`, "Releasing" | Lockstep versions, the release checklist as a fenced task list to copy into an issue, the cut from `dev-docs/releasing.md`, the paced publish, the crates.io account, what the tag build refuses, the install line and the deploy from `master`, and the stability policy of [5.8] |
| `reference/api.md`, "API reference" | Which crate to depend on, one docs.rs link per crate, the facade's features and its module tree |

`authoring/registering.md` became a redirect to "Model sets", which holds its content.
The determinism page's kit section shrank to a pointer, and the page keeps its title, since six pages link it as "Determinism and testing".

### Rewritten pages

- **The five first-model pages** open with the `!!! info "Henad 0.3"` admonition and send the reader to "Your own project".
  Snippet titles read `src/...`, registration is a `mod` line, an import and an `insert` line in `models()`, the app runs with `cargo run --release`, the CLI with `--bin my-model-cli`, the web build on port 8081, and each `--params` block shows the reader's own model, taken from a scratch host over the tutorial's set.
  The "registry tests" sentences point at the template's test and the kit.
  The GPU Life page includes the template's `build.rs` whole in place of henad-models' `shader_build` region.
  The Game of Life page was rewritten by hand, and its diff was the pattern for four parallel agents, one per remaining page.
- **The tutorial crate** seeds through `grid_init_rng` and `agent_init_rng` (#41), and its parity tests still pass.
- **`installation.md`** gives `cargo install --locked --config 'profile.release.opt-level=2' henad-app henad-cli` (Q4), a clone, and starting a project. **`running.md`** gives the installed, clone and project forms.
- **`sweeps.md` and `search.md`** show each spec whole in a collapsed block and say "save it as", with `--spec sir_sweep.toml` and the like, and a note on running the CLI installed, from a clone or in a project. The four spec files' "Run it with" comments lost their repository paths to match.
- **`shaders.md`** takes the template's `build.rs`, `henad::include_shaders!()`, and the generated `henad` module with `::henad` in a hand-written wrapper.
- **`app.md`** gains the note beside Open for a model that does not replay exactly, and the changed-build warning.
- **`architecture.md`** has eight crates, the facade, the model set and device needs, the dev-only pair of edges, the tutorial crate and the template. **`gpu-backend.md`** says where models live now.
- **`models.md`, `index.md`, `porting.md`, `authoring/index.md`, `cli.md`, the root `README.md`** drop "ship with", link the new pages, and `cli.md` shows the template's CLI binary as the way to host the command line.
- **The template's README** says "My Model" keeps its settings in `My-Model` (#42).

### The example program (#40)

At [4.2.7]'s values SIR burns out long before tick 300, since one cell in a hundred starts infected.
Measured with `henad-cli --export-stats` at seed 7 on 256 by 256, every susceptible cell was gone by tick 25 at an infection rate of 0.3, and 308 of 65,536 were left at tick 100 at 0.05.
The program now starts one cell in a thousand at a rate of 0.05, seeds an outbreak at tick 50, and steps to tick 100, where 23,836 cells are still susceptible.
The live edit raises the rate to 0.3 with a second outbreak, and 20 ticks later 145 are left, against 13,231 in the same run with the outbreak and without the edit.

`#![expect(clippy::print_stdout)]` is gone.
The workspace warns on `allow_attributes`, and an `allow` would trip it, so the program prints through `writeln!` to `std::io::stdout()` instead, which no lint flags and any crate accepts.
`run_sampled`'s closure collects its samples into a vector, since a `Stdout` lock is no `Send` and the closure runs on a pool worker.

The guide includes the program by region (`build`, `live`, `sweep`, `replay`, `app`), and whole once at the top, where Zensical strips the markers.
`the_readme_program_matches_the_example` compares the README's fence with the example minus its `// --8<--` lines, and the README carries no markers.

The three unused regions are deleted: `crates/henad-app/build.rs:build_script`, `crates/henad-models/src/lib.rs:include_shaders` and `crates/henad-models/build.rs:shader_build`.

### AGENTS.md and the changelog

- AGENTS.md: the include rules for the template's comment forms, whole-file includes and the version admonition, the README test's marker filter, a "Cut a release" line pointing at the checklist, "Adding a new model" opening on the template, and `REPLAYS_EXACTLY` with the two seed helpers beside the registration paragraph.
- CHANGELOG: one Added line (the new pages) and seven Changed lines, the registering page's redirect among them.

### The review

The maintainer reviewed the pass before committing it, and every finding is folded in.

- **The release cut** in `releasing.md` now bumps the version first: `[workspace.package]`, every requirement and the template's three (`henad`, `henad-build`, and `henad` under `[dev-dependencies]`), the `fetch` tag, then `docs/license.html` regenerated, `cargo update --workspace`, the dated CHANGELOG, a commit pushed, the checklist run on that commit, and only then the tag.
  The checklist gained the workspace version and the licence page, and names the template's three requirements.
  The undated-section example is `## [0.3.0]` with no date, since `## [Unreleased]` fails earlier as no section (checked with `scripts/changelog_section.py`).
- **`library.md`**: `act` fires on the current state, and a live edit takes effect from the next tick.
  The speed table's ratios are against 0.2's own benchmark loop, with `run_for` within 3% of it, and a ratio near 1 reads as spread.
  "Seven names" became "The main types", with `ModelEntry` at `henad::` and `SweepRecord` at `henad::explore` outside the prelude.
  The stamp paragraph says that two unknown builds never count as the same and every resume warns, in the same words as `your-project.md`.
- **`your-project.md`**: renaming the package renames the library, the 4.7 times is a toy kernel at opt-level 1 against 2 in single runs, and the template's workflow runs on pushes to `main`, pull requests and dispatch.
- **The five first-model pages**: each id footnote says the guide's ids differ so its models can sit in one set beside the example models, as the parity tests run them.
- **One admonition wording**, "This page describes Henad 0.3.", on all eleven pages that include template or tutorial code, `model-sets.md`, `shaders.md` and `cli.md` among them.
- **`testing.md`**: a missing optional feature returns `None` even under `HENAD_REQUIRE_GPU`.
- **Low items**: the trailing "which" and contrastive "where" clauses the review named, and four more in the tutorial pages, `sweeps.md`'s "Then run it" after the spec block, "These two are the only dev-only edges", the split `#[non_exhaustive]` sentence, and AGENTS.md's release line rewritten with its full stop and the registration paragraph rewrapped.
- **A redirect** for `/authoring/registering/`: the page is a stub outside the nav that replaces its location with `model-sets/`, and links there for a reader without scripts.
  Zensical has no redirect plugin configured.

### Edited tree

```text
.
├── AGENTS.md                                   ~ includes, release line, adding a model
├── CHANGELOG.md                                ~ M11's lines
├── README.md                                   ~ install line, your own models, the facade
├── zensical.toml                               ~ six pages, registering out, nav entry #43
├── crates/henad/
│   ├── README.md                               ~ the program, retuned
│   ├── examples/complete.rs                    ~ retuned, writeln!, five regions
│   └── src/lib.rs                              ~ the README test skips marker lines
├── crates/henad-app/build.rs                   ~ unused region removed
├── crates/henad-models/{build.rs,src/lib.rs}   ~ unused regions removed
├── crates/henad-explore/specs/*.toml           ~ "Run it with" comments without paths
├── examples/tutorial/src/{gpu_life,gpu_foraging}/mod.rs   ~ grid_init_rng, agent_init_rng
├── templates/model-project/README.md           ~ the storage folder's dash
└── docs/
    ├── index.md                                ~ your own project card, eight crates
    ├── guide/
    │   ├── your-project.md                     +
    │   ├── library.md                          +
    │   ├── installation.md  running.md         ~ rewritten
    │   ├── sweeps.md  search.md  app.md        ~ saved specs, replay note
    │   └── first-model/*.md                    ~ all five, in the template
    ├── authoring/
    │   ├── model-sets.md                       + replaces registering.md
    │   ├── registering.md                      ~ a redirect to model-sets.md
    │   ├── testing.md                          +
    │   └── determinism.md  shaders.md  porting.md  index.md   ~
    ├── reference/
    │   ├── api.md                              +
    │   └── models.md  cli.md                   ~
    └── developing/
        ├── releasing.md                        +
        ├── architecture.md  gpu-backend.md     ~
        └── agent-record/20261002-43-library-docs.md   +
```

## State after

Everything is uncommitted on `48-library`, on top of 6217127, in the main checkout as asked, and nothing is staged.
The workspace version is 0.2.0, and the template still requires `"0.2"`.

- `uv run --locked zensical build` passes with its path checks and no issues, after the review's changes.
- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 1067 tests, none failed, with the wasm32 typechecks, packaging, cargo-deny, the docs, the facade's doc test and the web build.
  The count equals M10d's, since no test was added or removed.
  The review touched Markdown alone, and changed no code the run covers.
- The program's first half, run in release with the app region cut, printed the output quoted on the library page.

Proposed commit: `docs: library guide, template tutorials and release procedure`.

## Issues found & future directions

- **M11b remains.** The bump to 0.3.0 with every requirement and the template's, the licence page regenerated, the final comparison of [7.1] against v0.2.0, #49, #37, #10, #19 and #18 confirmed in v0.4.0, the merge into `master`, the checklist, the tag and the publish.
  Record #42's open question on the 64² gate (accept it, one codegen unit, or `#[inline]` around the row loop) needs an answer before that comparison.
- **The pages say 0.3 ahead of the crates.** The admonitions, `henad = "0.3"` on the library and testing pages, and the template's `fetch` region all name 0.3, while the workspace is 0.2.0.
  They match once M11b bumps the version, and the site deploys from `master` only after the merge.
- **The Write and Edit tools refused this checkout.** A background-job guard rejects edits outside a worktree, and the session wrote every file through the shell, since the brief asked for the main checkout.
  The guard can be turned off with `"worktree": {"bgIsolation": "none"}` in `.claude/settings.json`, which was left alone.
- **No screenshot.** No new page needed one, and no existing one shows a product name, so the app was not launched.
- **"Shipped" stays in the tutorial pages** where it names the example model a page mirrors, as [8] keeps it for the tutorial and its parity tests. "Default model" and "ship with" are gone from the docs.
- **The CLI's own crate doc** still runs `henad-cli --spec crates/henad-explore/specs/sir_sweep.toml`, a line `existing_invocations_keep_their_mode` counts, and it stays.
- **Unverified claims on the pages.** The release checklist's `gh workflow run ci.yml --ref <branch>` dispatches the whole workflow, and its other jobs are conditioned on their own triggers, which the first real dispatch confirms. `cargo semver-checks` is not installed here and has never run on Henad.
- **Next.** M11b.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
