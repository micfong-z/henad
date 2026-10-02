---
date: 2026-10-02
title: "Library M10d: the template"
description: The tenth milestone of #48, fourth part. templates/model-project is the project a user fetches from a release and builds on the published crates, with a CPU and a GPU model of the voting rule, its own app, command line, web build, test and CI. Henad gains the downstream CI job that builds it against the packaged crates, actionlint over its workflow, and a check that its release profile equals the root's.
icon: material/content-copy
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M10d implemented, every downstream step passing by hand, the gate missed at 64² against a build on the workspace's lock and met against the fresh lock and at one codegen unit
baseline_commit: f4af780
delta_state: uncommitted on `48-library`, on top of M10c's commit
---

# Library M10d: the template

> M10d is the fourth part of the tenth of the eleven milestones of #48, which turns Henad into a library published on crates.io.
> `templates/model-project` is a project of its own, outside the workspace, that depends on `henad` and `henad-build` alone.
> It holds `vote` and `gpu_vote`, the app and the command line over the facade, a test that runs the testing kit over every model it registers, a sweep spec, the threaded web build, `scripts/ci.sh` and a CI workflow that requires the GPU checks on lavapipe.
> Henad's CI gains the `downstream` job, started by hand, which builds a copy of the template against the crates as `cargo package` verifies them, and every one of its steps passed when run by hand.
> The performance gate missed at 64² against a template build on the workspace's own lockfile, twice, and met at every other point.
> The kernel compiles to different machine code in the two crates at the default 16 codegen units, and to the same code at one, where the gate is met.
> An edit to a model rebuilds both binaries in under a second in the dev profile and in about 1.3 s in release, and an empty commit costs the same.

## State before

`48-library` stood at f4af780 (M10c, the tutorial crate), with a clean tree.
`templates/model-project/` held only `scripts/web-toolchain` and `scripts/trunk-version`, placed there in M1.
The root `exclude` named `benchmarks/krabmaga` alone, and the CI `test` job carried the lavapipe download inline.
`examples/tutorial` had shown the import shape the template follows: `henad::authoring::prelude::*`, the root macros, `stamp_commit` with `ShaderBuild::discover("src")`, and `henad::include_shaders!()`.

## What was done

### The template

Every file of [4.3.2], plus `rustfmt.toml`.

| File | Holds |
|---|---|
| `Cargo.toml` | Package `my-model`, `publish = false`, `default-run`, the features `app` and `cli` (both default) over `henad/app` and `henad/cli`, two `[[bin]]` entries with `required-features`, `henad` and `henad-build` at `"0.2"`, `henad` with `testing` as a dev-dependency, `unsafe_code = "deny"`, and the profile block of [4.3.3] inside a `profile` region |
| `build.rs` | `stamp_commit()`, then `ShaderBuild::discover("src")?.generate()?`, then `Ok(())` |
| `src/lib.rs` | `mod gpu_vote; mod vote;`, `henad::include_shaders!()`, one `use` line per `register_*`, `models()` with one `insert` statement per model, and `every_model_conforms`, which names no model |
| `src/vote.rs` | `Vote`, the `GridModel` of [4.3.4] as written there |
| `src/gpu_vote/mod.rs` | `GpuVote`, a `GpuGridModel` with one `u32` per cell, seeded through `Vote::init` from `grid_init_rng(seed)` |
| `src/gpu_vote/{step,display,reduce}.wgsl` | The 3x3 vote through `henad::space::{cell_index, offset_cell, TORUS}`, a display pass through `henad::dims::{Dims, cell_at}`, and a workgroup-local count of the ones |
| `src/main.rs`, `src/bin/my-model-cli.rs` | The app and the CLI of [4.3.5], product name "My Model", `cli_command` and `command_name` `my-model-cli`, and the CLI's empty wasm32 `main` |
| `.cargo/config.toml` | The nine wasm32 rustflags of Henad's `scripts/build_web.sh` under `[target.wasm32-unknown-unknown]` |
| `index.html`, `Trunk.toml` | The page of [4.3.5] with its `crossOriginIsolated` note, and `filehash = false`, the two headers and port 8081 |
| `rust-toolchain.toml`, `rustfmt.toml` | 1.97 with the wasm32 target, and `max_width = 120` |
| `scripts/build_web.sh` | Refuses a set `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS`, even an empty one, naming `.cargo/config.toml`, prints the nightly's install line when the toolchain or `rust-src` is missing, then runs Trunk with `RUSTUP_TOOLCHAIN` and `CARGO_UNSTABLE_BUILD_STD` |
| `scripts/ci.sh` | The stages `lint`, `lint-web`, `test` and `web`, all four without an argument, `--locked` once a lock exists. `web` reads the binary's name from `data-bin` in `index.html`, so a rename needs no edit here |
| `scripts/install-lavapipe.sh` | The Mesa 26.1.3 and `build29` pins, the loader and `vulkaninfo` from apt, the driver manifest, a Vulkan summary, and the two variables into `$GITHUB_ENV`, or printed as `export` lines outside Actions |
| `specs/vote.toml` | A factorial over five densities at 256², four replicates, 20 runs |
| `.github/workflows/ci.yml` | The nightly, Trunk and lavapipe installs, then the four stages, the test stage under `HENAD_REQUIRE_GPU=1`, with the coi-serviceworker note on GitHub Pages |
| `.github/dependabot.yml` | Cargo with `henad` and `henad-build` in one group, and GitHub Actions |
| `.gitignore` | `/target`, `/dist`, `.DS_Store`, `*.swp`, `*~`, and never `Cargo.lock` |
| `README.md` | The `fetch`, `rename`, `run` and `update` regions, `--release` for a model run, committing the lock, adding a model, the web build, the checks |
| `assets/icon-256.png` | A 32 by 32 field after six steps of the voting rule, in the model's two colours, scaled by 8 |

No `Cargo.lock` ships, and none sits in the tree.

`vote` and `gpu_vote` agree tick for tick.
`--export-stats --seed 1 --steps 300` at 200 by 150, a width that is no multiple of the workgroup, wrote the same bytes from both.
The kit passes both: `vote` skips the four GPU checks and `gpu_vote` skips `ThreadCount`, each as not applying to its backend, and nothing else is skipped.

#### Where the template departs from the design's text

- **The requirements read `"0.2"`.** [4.3.3] prints `"0.3"`, and M11 moves "the template's requirements with it" to 0.3.0.
  Until then the template requires the workspace's major and minor, as [5.5]'s checklist asks.
- **`build.rs` ends in `Ok(())`.** [4.3.3] tells a crate without shaders to drop the `ShaderBuild` line.
  With the design's shape, where `generate()` is the tail expression, that deletion leaves a function with no value.
  Here it deletes one line, as the `downstream` job's last step does.
- **One `insert` statement and one `use` line per model** in place of a chained `insert(..)?.insert(..)?;` and a braced import.
  Removing a model is then whole lines, and the job's last step removes `gpu_vote` with line deletions alone.
- **`rustfmt.toml`** is a file [4.3.2] does not list.
  The design's template code is written at Henad's 120 columns, and rustfmt's default of 100 would rewrap eight places.
- **`README.md` adds `run` and `update` regions** beside `fetch` and `rename`, and `Cargo.toml` a `profile` region, for the guide pages of [8] to include.
- **`main.rs` and the CLI binary carry a one-line `//!`** each.
- **`scripts/ci.sh test` also runs `cargo test --doc`.** `--all-targets` leaves doc tests out, and a model library might grow some.

### Henad's side

- **Root `Cargo.toml`**: `exclude = ["benchmarks/krabmaga", "templates/model-project"]`, with the comment saying the template is a project of its own that the `downstream` job builds.
- **`scripts/check_packaging.sh`** compares the template's `[profile.release]` table with the root's and names both on a mismatch.
  A planted `opt-level = 3` in the template failed it, and the revert passed.
- **CI `lint`** downloads actionlint 1.7.12 and runs it over `templates/model-project/.github/workflows/*.yml`.
- **CI `test`** calls `templates/model-project/scripts/install-lavapipe.sh` on Linux.
  The inline download, the separate Vulkan summary step and `vulkan-tools` in the Linux deps go, since the script installs the loader and prints the summary.
- **CI `downstream`**, on `workflow_dispatch`, has the 15 steps of [5.4] in order.
  It takes no rust-cache, since the packaging needs fresh target directories.
  Points of detail:
  - The patch block names the seven crates the template resolves. henad-models is left out, since an unused patch entry prints a warning on every command.
  - Step 3's `jq` asserts both that `henad` and `henad-build` are present and that every `henad*` package has a null `source`.
  - Step 7 fails on any `Dirty`, `Compiling` or `Running` line of the second build.
  - Step 10 also asserts that `dist/` does not exist after the refusal.
  - Step 12 also asserts that `my-model` did recompile.
  - Step 13 builds, runs clippy under `-D warnings`, and greps the generated `shader_bindings.rs` for `pub mod probe` and `binding_decls.rs` for `PROBE_UNUSED`.
  - Step 15 also removes step 13's probe shader, asserts `stamp_commit` is still in `build.rs`, and counts four rows in `runs.csv` after the resume.

actionlint 1.7.12 with shellcheck 0.11.0 passes the template's workflow.
Over Henad's own `ci.yml` it finds one SC2155 at line 64, in the nightly clippy step that predates this session, which CI does not lint.
A first pass found SC2054 on the sweep array of step 15, and the density list is now quoted.

### The downstream job, by hand

The job's YAML was parsed and each `run` block executed verbatim under `bash --noprofile --norc -eo pipefail`, GitHub's default shell, with `RUNNER_TEMP` a scratch directory outside any git work tree and `GITHUB_ENV` emulated.
Two deviations from a runner: the three install steps were skipped, since the nightly and Trunk 0.21.14 were installed and macOS steps the GPU checks on Metal, and `cargo package` took `--allow-dirty` on the uncommitted tree.
Both packaging passes ran in fresh `CARGO_TARGET_DIR`s, as record #34 asks.
Apple M4 Pro, rustc 1.97.1 (8bab26f4f 2026-07-14).

| Step | Result | Time |
|---|---|---|
| 1. Installs | Skipped, as above | |
| Verified packaging, both passes | Pass, eight crates verified, then the facade with all four features | 156 s |
| 2. Copy, `=0.2.0`, patch, `generate-lockfile` | Pass, three requirements rewritten, 459 packages locked | 4 s |
| 3. The patch is used | Pass, `true` | 1 s |
| 4. henad-models absent | Pass | 0 s |
| 5. Strict clippy | Pass, `-D warnings -D unreachable_pub -D unused_qualifications` | 41 s |
| 6. `ci.sh test` under `HENAD_REQUIRE_GPU=1` | Pass, `every_model_conforms` in 0.40 s | 135 s |
| 7. Second `cargo build -v` | Pass, 543 `Fresh` lines and nothing else, 0.16 s | 16 s |
| 8. wasm32 library, `--no-default-features`, `RUSTFLAGS=""` | Pass | 97 s |
| 9. `ci.sh lint-web` | Pass | 60 s |
| 10. `RUSTFLAGS='-C opt-level=1' build_web.sh build` | Refused with exit 1, naming `.cargo/config.toml`, no `dist/` | 0 s |
| 11. `ci.sh web` | Pass, `dist/my-model.js` holds `wbg_rayon_start_worker` and `initThreadPool`, and the worker helpers exist | 56 s |
| 12. Comment appended to `src/vote.rs` | Pass, `Dirty my-model: the file src has changed`, every henad crate `Fresh` | 2 s |
| 13. Unreferenced shader in `src/probe/` | Pass, module and constant generated, clippy clean | 3 s |
| 14. CLI dry run of `specs/vote.toml` | Pass, 5 configs by 4 replicates, 20 runs, 7 lanes of 2 threads | 1 s |
| 15. CPU-only crate, sweep and `--reps 2` resume | Pass, 2 runs skipped and 2 run, no `explore_warning` | 2 s |

Step 15's check can fail.
After the job, an appended comment in `vote.rs`, a rebuild and a `--reps 3` resume printed a `build_changed` warning of role `model`, naming source hashes 4ad6b3dc7c13b052 and 0553c874e621aff0, and the step's `jq` filter selected it.

### The app

The template's dev-profile app opened on the second monitor, titled "My Model", with `vote` selected and its three parameters.
eframe stores it under `~/Library/Application Support/My-Model`, the space turned into a dash, as 0.2.0's "Henad Engine" went to `Henad-Engine`.
The folder did not exist before the launch, and was removed after it.
The first launch opened on the main display for about ten seconds, since the window position had been seeded under `My Model` with a space.
It was closed, reseeded under `My-Model`, and reopened on the second monitor.
The facade does not expose henad-app's `inspection` feature, so the check went through `CGWindowListCopyWindowInfo` and `screencapture` in place of the egui MCP server.

### The gate

A is `life`, the tutorial's Game of Life, built in the workspace as a temporary example of henad-tutorial over `henad::cli::run`, removed after the build.
B is the same `life.rs` copied into a template copy outside git, registered in its `models()`, built against the packaged crates.
Both are release builds at opt-level 2.
`--export-stats --seed 1 --steps 300` at 64² and 1024² wrote identical bytes from A and from every B below.

The template's fresh lock differs from the workspace's in 161 of 403 shared packages, rayon 1.12.0 against 1.11.0 among them.
B was therefore built twice: once on the lock a new user's first build writes, and once on a lock seeded from the workspace's `Cargo.lock`, which then matched it in all 406 shared packages.

Each configuration ran `--json --seed 1 --steps 1000 --warmup 200 --reps 5` in ten rounds of A, B, B, A, after 120 s idle and with 30 s between configurations, pairing each repetition of B with the same repetition of the A beside it, 100 pairs a configuration.
Nothing else was built or run during a timing.
The longest configuration took 13.6 s, far under the cap of 1000 s.
The machine was the maintainer's working desktop, with Chrome and iTerm active.

| B | Load at start | 64² A median | 64² B median | 64² ratio | 1024² A median | 1024² B median | 1024² ratio |
|---|---|---|---|---|---|---|---|
| Workspace's lock | 4.97 | 1.64 ms | 1.78 ms | **1.087** | 51.3 ms | 54.5 ms | 1.002 |
| Workspace's lock, rerun | 1.69 | 1.52 ms | 1.61 ms | **1.066** | 44.2 ms | 45.2 ms | 1.038 |
| Fresh lock | 3.82 | 1.58 ms | 1.58 ms | 0.988 | 46.5 ms | 45.8 ms | 0.996 |
| Workspace's lock, both sides at `codegen-units = 1` | 4.52 | 1.67 ms | 1.71 ms | 1.013 | 47.6 ms | 49.2 ms | 1.024 |

The gate as specified, a template build against the same model in the workspace, misses at 64² against the build on the workspace's lock, on the first run and on the rerun the protocol asks for.
The 64² quartiles of the rerun run from 1.026 to 1.090.
It is met against the fresh lock, which is the build a user gets.
The 1024² ratios all pass, with quartiles as wide as 0.81 to 1.19 from run to run.

The cause is code generation, not the template's place outside the workspace.
The row kernel, `<&step_grid<LifeModel>::{closure#0} as FnMut>::call_mut`, compiles to 292 instructions in A, 304 in the workspace-lock B and 296 in the fresh-lock B, and `GridModelState::step` to 69, 65 and 65.
At one codegen unit on both sides, the kernel inlines into rayon's `bridge_producer_consumer::helper`.
That function is 420 instructions in both builds, the same apart from ten address immediates, `step` is identical, and the timings meet the bound.
At the default 16 units, the partition that receives the instantiated kernel depends on what else the instantiating crate holds (five models in henad-tutorial, three in the copy), and with it the inlining around the row loop.
The effect cuts both ways, since the fresh-lock B was 1.2% faster at 64².
64² is a single 4096-cell job of about 1.6 µs a step, where a few extra instructions in the row loop show, and 1024² spreads over every core.

### Rebuild times, #49's baseline

A git-tracked copy of the template on the fresh lock and the packaged crates, warmed in both profiles, then five rounds after 60 s idle, the profile order alternating each round.
Each round and profile appended a comment line to `src/vote.rs` and timed `cargo build --locked --bin my-model`, appended another and timed `--bin my-model-cli`, settled with a full build, committed the edits and timed a full build, then made an empty commit and timed a full build.
Every timed build recompiled `my-model` alone and reran its build script.
After an edit the trigger is `src`, and after a commit `.git/logs/HEAD`, which `stamp_commit` watches.
Times are wall seconds of the `cargo build` process, median of five, with the range.

| Profile | Edit, then the app binary | Edit, then the CLI binary | Commit of built edits, both binaries | Empty commit, both binaries |
|---|---|---|---|---|
| dev | 0.87 (0.83 to 2.05) | 0.82 (0.81 to 0.87) | 0.86 (0.85 to 0.90) | 0.86 (0.84 to 0.89) |
| release | 1.37 (1.32 to 2.02) | 1.20 (1.18 to 1.23) | 1.36 (1.35 to 1.55) | 1.35 (1.34 to 1.48) |

The two maxima of 2 s are each profile's first round after the warm-up builds.
The times are short because the template crate is small: two models, the generated shader bindings, and two thin binaries, with every Henad crate `Fresh`.
They were taken on the maintainer's working desktop and are indicative.
A commit with no source change costs as much as an edit, since both rerun the stamp and recompile the crate holding the kernels.

### AGENTS.md and the changelog

- AGENTS.md: the template beside `benchmarks/krabmaga` in the exclude, the packaging line naming the profile check, the CI-only paragraph naming `actionlint` and the `downstream` job step by step with how to run it by hand, a bullet for the template after the tutorial crate's, and the lavapipe script under `HENAD_REQUIRE_GPU`.
- CHANGELOG: three Added lines (the template, the `downstream` job, actionlint and the profile check) and one Changed line (the `test` job's lavapipe).

### Edited tree

```text
.
├── AGENTS.md                               ~ the template, the downstream job, the exclude, the profile check
├── CHANGELOG.md                            ~ the template, downstream, actionlint, lavapipe
├── Cargo.toml                              ~ templates/model-project in exclude
├── zensical.toml                           ~ nav entry #42
├── .github/workflows/ci.yml                ~ actionlint in lint, lavapipe script in test, + downstream
├── scripts/check_packaging.sh              ~ the template's [profile.release] against the root's
├── templates/model-project/                + the template (scripts/web-toolchain, trunk-version from M1)
│   ├── Cargo.toml  build.rs  README.md  .gitignore  rust-toolchain.toml  rustfmt.toml
│   ├── index.html  Trunk.toml  .cargo/config.toml  assets/icon-256.png  specs/vote.toml
│   ├── scripts/{build_web,ci,install-lavapipe}.sh
│   ├── src/{lib,main,vote}.rs  src/bin/my-model-cli.rs
│   ├── src/gpu_vote/{mod.rs,step.wgsl,display.wgsl,reduce.wgsl}
│   └── .github/{workflows/ci.yml,dependabot.yml}
└── docs/developing/agent-record/20261002-42-library-template.md   +
```

## State after

Everything is uncommitted on `48-library`, on top of f4af780, in the main checkout as asked.
Nothing is staged.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 1067 tests, none failed, with the wasm32 typechecks, packaging (the new profile check included), cargo-deny, the docs and the web build.
  The count equals M10c's, since no workspace code changed.
- `uv run --locked zensical build` passes with no issues.
- Every step of the `downstream` job passed by hand, as tabled above.
- actionlint 1.7.12 with shellcheck 0.11.0 passes `templates/model-project/.github/workflows/ci.yml`, and shellcheck passes the three template scripts.
- A copy of the template passes `cargo fmt --check` under its own `rustfmt.toml`.
- `templates/model-project/Cargo.lock` does not exist, and nothing under the template is ignored but `target` and `dist`, which no build in the tree wrote.

Proposed commit: `feat: model project template`.

## Issues found & future directions

- **The gate's miss needs a decision.** Three courses are open.
  The first accepts it.
  The template meets the bound on the lock a user gets, and the miss is per-crate codegen that can go either way.
  The second sets `codegen-units = 1` in both `[profile.release]` tables.
  `check_packaging.sh` already holds the two equal.
  Each model then compiles to one kernel whatever crate holds it, at a cost in release build time nobody has measured.
  The third marks the engine's functions around the row loop `#[inline]`, so that a copy lands in whichever codegen unit calls them.
  That might make the kernel's shape independent of the partition, and it needs its own measurement against [7.1]'s gate set.
  [7.1]'s "No difference is expected, since the template's release opt-level matches the root's" holds for the opt-level and misses the partition.
- **Linux is unverified.** The job ran on macOS.
  The first dispatched run will settle `install-lavapipe.sh` on a runner, the inference of [4.3.6] that the template links on Ubuntu without the X11 and xkb packages, and actionlint's shellcheck pass as it runs there.
- **The Dependabot group** cannot be checked from Henad's repository, since Dependabot reads only a repository's root `.github`.
  It needs a project made from the template, or the first release.
- **eframe names the storage folder `My-Model`**, with a dash.
  The README says only that the product name names the folder.
- **SC2155 at line 64 of Henad's `ci.yml`** predates this session.
  CI lints only the template's workflows, and the finding stays.
- **The downstream job's packaging needs no X11 packages on macOS**, and [5.5]'s second pass compiles henad-app's binary on Linux too.
  If the first Linux run fails to link there, the job needs the Linux deps step the other jobs carry.
- **Next.** M11 writes the guide pages that include the template's regions, moves the requirements to 0.3, and runs the final comparison of [7.1].

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
