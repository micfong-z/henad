---
date: 2026-10-01
title: "Library M1: packaging hygiene"
description: The first milestone of #48, which makes every crate package on its own. It adds caret requirements between the crates, per-crate READMEs, metadata and licence copies, moves the app's assets into its crate, rebuilds the IBM Plex fonts under new names, pins the web build's nightly, and adds packaging, MSRV, docs and cargo-deny checks. The performance protocol's baseline phase ran first.
icon: material/package-variant-closed
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M1 implemented, `./check.sh` with HENAD_REQUIRE_GPU=1, the docs build and the web gate green
baseline_commit: 773a7a5
delta_state: uncommitted on `48-library`
---

# Library M1: packaging hygiene

> Issue #48 turns Henad into a library published on crates.io, and its design splits the work into eleven milestones.
> This session ran the baseline phase of the design's performance protocol, then implemented M1, packaging hygiene.
> The baseline built v0.2.0's `henad-cli` at opt-level 2 and 3 and compared them on the CPU half of the gate set, for the maintainer's choice of the installed opt-level (Q4).
> M1 makes each crate package on its own.
> Every Henad crate now requires its siblings at the workspace version, carries its own README, metadata and licence texts, and reads no file outside its directory, apart from henad-models' shader build, which M5 replaces.
> The app's two IBM Plex fonts were rebuilt from the same upstream files under the names Henad Sans and Henad Mono, through a procedure written beside them, and draw every character as before.
> The web build now reads one dated nightly from `templates/model-project/scripts/web-toolchain`, and CI gained packaging, MSRV, docs and cargo-deny checks.

## State before

Henad 0.2.0 was tagged at 773a7a5, and `48-library` sat on that commit with a clean tree.
The six crates required each other by path alone, and `cargo package` refused every crate but henad-core.
Every crate shipped the root README, whose images are relative paths, and its `documentation` key pointed at the guide.
henad-app embedded its fonts, icon and logo from the root `assets/` folder through `../../../assets` paths, outside its package.
Two of those fonts were FontFreeze derivatives of IBM Plex Sans and Mono that still carried the name "Plex", which IBM's font licence reserves for unmodified versions.
Two henad-cli tests read files outside the package, `docs/reference/cli.md` through `include_str!` and a henad-explore spec by path.
The web build installed whatever nightly was current, in CI, on Vercel and on a contributor's machine.
`deny.toml` existed, and nothing ran it.

## What was done

### Baseline phase

The design's performance protocol starts with a baseline, B0, a release `henad-cli` built from the v0.2.0 tag, which is 773a7a5.
The tag was exported with `git archive` outside the repository and built twice, at the workspace's opt-level 2 and with `--config 'profile.release.opt-level=3'`, both on rustc 1.97.1 (`8bab26f4f 2026-07-14`, LLVM 22.1.6, aarch64-apple-darwin, an Apple M4 Pro with 14 cores).
Before any timing, `--export-stats --seed 1 --steps 300` wrote identical bytes from both builds for all six CPU models.

The CPU half of the gate set ran through `henad-cli --json --seed 1 --steps 1000 --warmup 200 --global-warmup 1000 --reps 3`, in two ABBA blocks per configuration.
Rep `i` of each invocation has seed `1 + i`, so rep `i` of opt-level 2 and rep `i` of opt-level 3 form a pair, 12 pairs per configuration.
The table gives the median of the paired ratios, opt-level 3 time over opt-level 2 time, so a ratio above 1 means level 3 is slower.

| Configuration | Level 2 median rep | Median ratio, 3 over 2 | Range of ratios |
|---|---|---|---|
| game_of_life 64² | 1.7 ms | 1.053, rerun 1.062 over 48 pairs | 1.042 to 1.099, rerun 0.999 to 1.114 with 1 of 48 below 1 |
| game_of_life 1024² | 57 ms | 1.001 | 0.956 to 1.025 |
| game_of_life 4096² | 254 ms | 0.998 | 0.911 to 1.085 |
| sir 64² | 1.9 ms | 1.140, rerun 1.054 over 48 pairs | 0.690 to 1.456, rerun 0.704 to 1.503 with 17 of 48 below 1 |
| sir 1024² | 99 ms | 1.024 | 0.885 to 1.081 |
| boids 1,000 | 476 ms | 0.990 | 0.957 to 1.021 |
| boids 50,000 | 22.3 s | 1.014 | 0.693 to 1.064 |
| ants 1,000 | 148 ms | 0.996 | 0.884 to 1.056 |
| ants 1,000,000 | 13.8 s | 1.001 | 0.967 to 1.023 |
| virus_network, default 10,000 nodes | 38 ms | 1.023 | 0.717 to 1.046 |
| team_assembly, default | 0.5 ms | 0.980 | 0.909 to 1.091 |

Boids and ants ran at the model's default density, with the world scaled as `scripts/bench_matrix.py` scales it.
No configuration came near the 1000-second cap. The longest, boids at 50,000, took about 13 minutes for both builds together.
The two 64² grids were rerun with eight blocks, 48 pairs, since their reps last about two milliseconds.

The numbers are indicative only.
The machine was not quiet: another process held one core at 100% throughout, and the load average stayed between 9 and 19 on 14 cores.
Read with that caveat, opt-level 3 is no faster anywhere.
It is about 6% slower on Game of Life at 64², where the ratio stayed above 1 in 47 of 48 pairs, and probably slower on SIR at 64², whose pairs scatter too widely to say by how much.
Every other configuration lies within 2.5% of 1.
The repository's earlier record had level 3 at +1.3% on Game of Life and -4.8% on boids (`benchmarks/method.md`).
The maintainer chooses the install line of Q4 from these numbers.

### Packaging

- **Version requirements.** Each `henad-*` entry of `[workspace.dependencies]` carries `version = "0.2.0"` beside its path (decision 2.1).
  The workspace's `homepage` is now the guide, and `documentation` and `readme` left `[workspace.package]`.
- **Crate metadata.** Each crate has a description, five keywords, its categories, its own README, `documentation` pointing at its docs.rs page, and a `[package.metadata.docs.rs]` table after the design's table of docs.rs settings.
  henad-cli's table leaves out `no-default-features`, since the crate has no features until M8.
  henad-models describes itself as the example models, excludes `tests/` with its 588 KB of consistency fixtures, and takes rayon as a dev-dependency, since only its tests build a pool.
- **Licence copies.** Every crate directory holds copies of `LICENSE-MIT` and `LICENSE-APACHE`.
- **READMEs.** Each crate's README is short, with absolute links into the guide.
- **Out-of-package reads.** henad-app's six `include_bytes!` paths and `index.html`'s icon copy point at `crates/henad-app/assets/`.
  `existing_invocations_keep_their_mode` reads `docs/reference/cli.md` at run time, and it and `a_search_spec_plans_its_budget_and_space` skip with a note when their file is absent, as in a crate built from its tarball.
- **`scripts/check_packaging.sh`.** It checks that every `henad-*` workspace entry carries the workspace version, that each crate's licence files equal the root's, that no `include_str!` or `include_bytes!` path resolves outside its crate, and that no build script joins a path climbing out of its crate.
  henad-models' build script is listed as the one allowed exception until M5.
  Run against the v0.2.0 tree, it reports the four bare path entries, the twelve missing licence copies, and all seven old include paths.
  The design asked only that an include outside `#[cfg(test)]` code be refused. The script refuses test includes too, which is simpler and catches nothing the tree still has.

### Fonts

The two IBM Plex fonts were FontFreeze derivatives that carried the reserved name "Plex".
Their name records named the upstream files: IBM Plex Sans 3.201 and IBM Plex Mono 2.3, the versions Google Fonts distributes.
Sans 3.201 exists only as Google Fonts' variable font `IBMPlexSans[wdth,wght].ttf`, the source of the shipped file, whose leftover style names list every width and weight of it.
IBM's own static releases are older, Sans 3.005 and Mono 2.004 or 2.005.
The rebuild therefore starts from Google Fonts' files at a pinned commit, as the shipped fonts did, in place of the design's static Sans file, which does not exist.

The procedure in `crates/henad-app/assets/fonts/SOURCES.md` cuts the regular instance (weight 400, width 100) with fontTools' instancer, then runs `pyftfeatfreeze -f ss01,ss02,zero -i -R ...` from opentype-feature-freezer 1.32.2.
`-R` left the old name in the unique identifier, name ID 3, and in the variable font's style-attribute names, so a short fontTools step drops the `STAT` table, renames every record but the copyright, trademark and licence records, and prunes the names nothing refers to.
`ttx -t name` then finds "Plex" only in the trademark record.
`SOURCE_DATE_EPOCH` fixes fontTools' timestamp, and two runs of the written procedure gave byte-identical files, whose SHA-256 hashes the note records.
Every code point of both new fonts draws with the same outline and advance width as in the files they replace, compared glyph by glyph through fontTools.

The Material Design Icons font matched the v7.2.96 webfont of Templarian/MaterialDesign-Webfont byte for byte.
Material Symbols Outlined reports version 2.667, and no commit of google/material-design-icons tried matched it, so the note records the version alone.
`OFL.txt` is the upstream licence text with IBM's copyright and Reserved Font Name line, and `LICENSE-APACHE` covers the two Material fonts.
`about.hbs` gained a static section for the four fonts, and the licence page was regenerated.

### Toolchain pins and CI

- **The pin.** `templates/model-project/scripts/web-toolchain` holds `nightly-2026-09-30`, and `scripts/trunk-version` beside it holds `0.21.14`.
  The design proposed nightly-2026-08-20, and asked that the date be no older than the nightly behind the current deployment.
  The build log of the production deployment of 773a7a5 on Vercel showed `rustc 1.101.0-nightly (5c543b0b8 2026-09-29)`, installed as the then-current nightly, so the pin is that build, whose channel date is 2026-09-30.
  `rustup toolchain install nightly-2026-09-30 --profile minimal --component rust-src,clippy --target wasm32-unknown-unknown` installed it on macOS. The Linux runners have not run it yet.
- **`scripts/build_web.sh`.** It reads the pin, checks the toolchain and its `rust-src` with `RUSTUP_AUTO_INSTALL=0`, prints the install command when either is missing, and unsets `CARGO_ENCODED_RUSTFLAGS`.
- **vercel.json.** It carries the install command, the pinned nightly with `rust-src` and Trunk at the pinned release, and the build command, `./scripts/build_web.sh build --release`.
  Both used to live in the project's settings, where the install command took whatever nightly was current and the newest Trunk.
- **CI.** `lint` runs `scripts/check_packaging.sh` and `cargo deny check`.
  New jobs on pull requests: `package` (`cargo package --workspace --exclude henad-tutorial --no-verify --locked` when a manifest or the lock changes), `msrv` (`cargo +1.95 check --workspace --locked`) and `docs` (the native docs under `-D warnings`, then henad-app's wasm32 docs on the pinned nightly with the atomics flags in both variables and a target directory of its own).
  The `web` job installs the pinned nightly and Trunk from the two files.
- **cargo-deny.** `spin` moved from the yanked 0.9.8 to 0.9.9, and RUSTSEC-2025-0057 (fxhash) and RUSTSEC-2024-0436 (paste) joined `[advisories] ignore` with their reasons.
- **check.sh.** It runs `scripts/check_packaging.sh`, `cargo deny` when it is installed, and the docs build under `-D warnings`.

### Rustdoc warnings

The design knew of henad-core's warnings alone.
`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --keep-going` found thirteen, in five crates.
Seven are `Self::` links in `gpu_grid_model.rs`, six in its module doc, where `Self` means nothing, and one naming `Self::BUFFERS.len()`.
Six link to private or unresolvable items, in `network.rs`, `gpu/sim_thread.rs`, `ants/mod.rs`, `cursor.rs` and two henad-app sweep panels.
The module doc's links now name `GpuGridModel`, `.len()` sits outside the link, and the other six are plain code spans.

### Edited tree

```text
.
├── .github/workflows/ci.yml          ~ lint: packaging, cargo deny. web: pinned nightly and Trunk. + package, msrv, docs
├── AGENTS.md                         ~ Commands, toolchain paragraph, licence page note
├── CHANGELOG.md                      ~ Unreleased
├── Cargo.toml                        ~ caret requirements, homepage is the guide
├── Cargo.lock                        ~ spin 0.9.9
├── README.md                         ~ pinned nightly
├── about.hbs                         ~ bundled fonts section
├── assets/                           - fonts/, icon-256.png, henad-logo-transparent-256.png
├── check.sh                          ~ check_packaging.sh, cargo deny, cargo doc
├── deny.toml                         ~ two advisories ignored with reasons
├── index.html                        ~ icon copied from the app's crate
├── vercel.json                       ~ installCommand, buildCommand
├── crates/
│   ├── henad-app/
│   │   ├── Cargo.toml                ~ metadata, docs.rs table
│   │   ├── LICENSE-APACHE, LICENSE-MIT, README.md   +
│   │   ├── assets/                   + icon-256.png, henad-logo-transparent-256.png (moved)
│   │   │   └── fonts/                + Henad Sans Regular.ttf, Henad Mono Regular.ttf (rebuilt),
│   │   │                               Material fonts (moved), OFL.txt, LICENSE-APACHE, SOURCES.md
│   │   └── src/                      ~ init.rs, main.rs, ui/about.rs include paths,
│   │                                   ui/sweep/footer.rs and parameters.rs doc links
│   ├── henad-cli/                    ~ Cargo.toml, + licences, README.md,
│   │                                   ~ main.rs and explore.rs tests skip without their files
│   ├── henad-compute/                ~ Cargo.toml, + licences, README.md, ~ gpu/sim_thread.rs doc link
│   ├── henad-core/                   ~ Cargo.toml, + licences, README.md,
│   │                                   ~ gpu_grid_model.rs and network.rs doc links
│   ├── henad-explore/                ~ Cargo.toml, + licences, README.md, ~ cursor.rs doc link
│   └── henad-models/                 ~ Cargo.toml (exclude, rayon a dev-dependency), + licences, README.md,
│                                       ~ ants/mod.rs doc link
├── docs/
│   ├── developing/contributing.md    ~ the pinned nightly, what CI runs beyond check.sh
│   ├── developing/agent-record/20261001-30-library-packaging.md   +
│   ├── guide/installation.md         ~ the pinned nightly and Trunk release
│   └── license.html                  ~ regenerated
├── scripts/
│   ├── build_web.sh                  ~ reads the pin, prints the install line, unsets CARGO_ENCODED_RUSTFLAGS
│   └── check_packaging.sh            +
├── templates/model-project/scripts/
│   ├── trunk-version                 +
│   └── web-toolchain                 +
└── zensical.toml                     ~ nav entry #30
```

## State after

M1 is implemented and uncommitted on `48-library`.
The workspace version stays 0.2.0, and every requirement names it.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 960 tests, none failed, with its new packaging, cargo-deny and docs steps, and the debug web build on the pinned nightly.
- `uv run --locked zensical build` passes.
- `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features` passes, and so does henad-app's wasm32 documentation on `nightly-2026-09-30` with the atomics flags in `RUSTFLAGS` and `RUSTDOCFLAGS`, without build-std.
- `cargo +1.95 check --workspace --locked` passes.
- `cargo package --workspace --exclude henad-tutorial --no-verify --locked` packages all six crates. henad-app's package is 4.4 MB compressed. The missing `henad-tutorial` is a warning only, and henad-models warns that it ignores its six excluded consistency tests.
- `cargo package --locked -p henad-core -p henad-compute` builds both from their tarballs.
  Local runs of both commands need `--allow-dirty` until the changes are committed.
- `cargo deny --locked check` passes: advisories, bans, licences and sources.
- **The M1 gate, the web check.** `scripts/build_web.sh serve --release` was opened with `?threads=8` in the desktop app's browser pane.
  The console showed no "thread pool init failed", eight workers started, `crossOriginIsolated` was true, and the System tab's Worker threads row read 8 of 14 logical CPUs.
  The release `dist/henad-app.js` holds `wbg_rayon_start_worker` and `initThreadPool`, and `dist/snippets/wasm-bindgen-rayon-*/src/workerHelpers.no-bundler.js` exists.
  The optional comparison against the 0.2.0 deployment was not run.

The browser pane's screenshots miss a WebGPU canvas and show a blank page.
A copy of the canvas taken with `toDataURL` inside `requestAnimationFrame`, laid over the page in an `<img>`, shows the app.

## Issues found & future directions

- **Q4 waits on the maintainer.** The baseline found no gain from opt-level 3 and a loss of about 6% on Game of Life at 64², on a loaded machine.
  A rerun on a quiet machine would firm up the small-grid numbers before the choice.
  It builds `cargo build --release -p henad-cli` from the v0.2.0 tag, once plain and once with `--config 'profile.release.opt-level=3'`, then runs the gate set's CPU half in ABBA order, pairing rep `i` of each build by its seed.
- **Departures from the design.**
  - The nightly is `nightly-2026-09-30`, not the proposed `nightly-2026-08-20`, which is older than the deployment's nightly. Its install on the Linux runners is confirmed only once CI runs.
  - The Sans rebuild starts from Google Fonts' variable font. The static Sans 3.201 file the design named does not exist.
  - The fonts' procedure adds a fontTools step after `pyftfeatfreeze`, since `-R` leaves name ID 3 and the style-attribute names.
  - `scripts/check_packaging.sh` refuses any include that climbs out of its crate, tests included.
  - Every crate gained its docs.rs table now, ahead of the docs.rs section's own milestone, so the `docs` job reproduces a configuration that exists. `#![cfg_attr(docsrs, feature(doc_cfg))]` and the `doc(cfg)` attributes are not added yet.
  - There were thirteen rustdoc warnings in five crates, not henad-core's alone.
- **The Vercel project skips preview deployments.** Its ignored-build step exits 1 only for production, so a pull request builds no preview, where the design says previews stay.
  The project's settings also hold the old install command, which vercel.json now overrides.
- **Rustup self-updated** to 1.29.1 when the pinned nightly was installed.
- **henad-models' package warnings.** `cargo package` warns once per excluded consistency test. `autotests = false` with explicit `[[test]]` entries would silence it, at the cost of six manifest entries.
- **Next.** M2, surface hygiene, which has no dependency, or M3 on top of M1.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
