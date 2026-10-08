---
date: 2026-10-02
title: "Library M11: the release"
description: "The release half of the last milestone of #48. The workspace moves to 0.3.0 with every requirement and the template's, the final comparison of the performance protocol passes against v0.2.0 for the checkout build and the installed form, and codegen-units stays at the default after one codegen unit measured slower."
icon: material/rocket-launch-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M11 complete, the final comparison passed on every gate configuration, 0.3.0 ready for the merge, the checklist, the tag and the publish
baseline_commit: 7faf864
delta_state: uncommitted on `48-library`, on top of M11a's commit
---

# Library M11: the release

> **Gate verdict: pass.** The final comparison of [7.1] against v0.2.0 passed on all 17 configurations of the gate set, for the 0.3.0 release build from the checkout and for the installed form (`cargo install` at opt-level 2), and the tag is not blocked.
> One configuration missed on its first run: sir at 64² against the checkout build read 1.069, then 0.992 on the rerun after cooling.
> The rest of the release half of M11 is done too: the workspace, every requirement and the template's three requirements moved to 0.3.0, the licence page was regenerated, `Cargo.lock` refreshed, and the CHANGELOG's 0.3.0 section was left undated for the maintainer to date at the tag.
> `codegen-units = 1` measured slower than the default 16 on three grid configurations, on the first run and on the rerun, by 6% to 17%, and the release profiles keep the default.
> M10d's template check at 64² still misses on a template build seeded from the workspace's lock, at 1.068 and 1.075, and passes on the fresh lock a user's first build writes, at 0.993.
> #49, #37, #10, #19 and #18 sit in the v0.4.0 milestone.

## State before

`48-library` stood at 7faf864 (M11a, the documentation), with a clean tree.
The workspace was at 0.2.0, and the guide, the admonitions and the template's `fetch` region already named 0.3, as record #43 left them.
Record #42 had left open how to answer the 64² miss of the template check: accept it, set one codegen unit, or mark the engine's functions around the row loop `#[inline]`.
The maintainer scoped the codegen-units course for this session, with the rule that one unit is adopted only if no gate configuration slows beyond the bound.

## What was done

### The version bump

The cut order of `docs/developing/releasing.md`, steps 1 to 4, short of the commit.

- **`Cargo.toml`**: `[workspace.package] version = "0.3.0"`, and `version = "0.3.0"` on all seven `henad-*` entries of `[workspace.dependencies]`.
  `scripts/check_packaging.sh` holds them equal and passes.
- **`templates/model-project/Cargo.toml`**: `henad = "0.3"`, `henad-build = "0.3"`, and `henad = { version = "0.3", features = ["testing"] }` under `[dev-dependencies]`.
  The `fetch` region of the template's README already named `v0.3.0`.
- **`Cargo.lock`**: `cargo update --workspace` moved the nine workspace packages to 0.3.0, henad-tutorial among them, and nothing else.
- **`docs/license.html`**: regenerated with cargo-about 0.9.1. The eight Henad crates read 0.3.0, and no other line moved.
- **`CHANGELOG.md`**: `## [Unreleased]` became `## [0.3.0]`, with a fresh empty `## [Unreleased]` above it.
  The section carries no date, and `python3 scripts/changelog_section.py 0.3.0` refuses it with "carries no release date", as the tag build will until the maintainer dates it.
  The link definitions read `[Unreleased]` as `compare/v0.3.0...HEAD` and `[0.3.0]` as `releases/tag/v0.3.0`, the form `[0.2.0]` uses.
- **`AGENTS.md`**: the template's requirements read `"0.3"`, in place of "`"0.2"` until M11 moves both".
  The ThreadCount note says the shared accumulator's test retries, after the review below.
- **`crates/henad-explore/src/tests/kit.rs`**: `a_shared_accumulator_fails_the_thread_count_check` retries up to five times, after the review below.

### The two doc lines

- `docs/guide/your-project.md`: the two lines claiming that the package renames alone, and that the paths in the binaries follow, are now one statement.
  "Renaming the package renames the library too, so the `my_model::` paths in `src/main.rs` and `src/bin/my-model-cli.rs` change with it, and every command keeps working."
  Every command keeps working because both `[[bin]]` entries and `default-run` name their binaries explicitly.
- `docs/guide/first-model/gpu-game-of-life.md`: "shipped port's own" became "example port's own".

### The milestone

`gh issue view N --json milestone` reads v0.4.0 for each of #49 (Dynamic model loading), #37 (Node-based editor for model authoring), #10 (GPU model authoring simplification), #19 (Custom UI widgets per model) and #18 (Model loading and authoring reworks), all open.

### The final comparison

#### The binaries

All on rustc 1.97.1 (`8bab26f4f 2026-07-14`, LLVM 22.1.6, aarch64-apple-darwin), on the Apple M4 Pro with 14 cores, each in a target directory of its own.

| Name | Built from | How |
|---|---|---|
| B0 | v0.2.0 (773a7a5), exported by `git archive` outside the repository | `cargo build --release --locked -p henad-cli` |
| 0.3.0 | The checkout at 0.3.0 | `cargo build --release --locked -p henad-cli` |
| Installed | henad-cli's tarball from `cargo package --workspace --exclude henad-tutorial`, unpacked | `cargo install --path <henad-cli-0.3.0> --locked --config 'profile.release.opt-level=2'`, with `patch.crates-io` pointing the five Henad crates it needs at their unpacked tarballs |
| 1 unit | The checkout at 0.3.0 | As 0.3.0, with `--config 'profile.release.codegen-units=1'` |

The installed form is the install line of `guide/installation.md` and releasing.md, as a crates.io install would run it.
The package is the root, no workspace profile reaches it, and `--locked` uses the `Cargo.lock` that `cargo package` wrote into the tarball.
The patches resolved under `--locked` without changing the lock.

#### Equivalence

Before any timing, `--export-stats --seed 1 --steps 300` at the size of every gate configuration wrote identical bytes from B0 and from each of 0.3.0, Installed and 1 unit, 51 comparisons in all.
gpu_boids compared its tick column, since it does not replay exactly.

#### The gate set

G as M6 ran it (record #35), with bench_matrix's settings.
Every run went through `henad-cli --json --seed 1`.
The CPU configurations ran `--steps 1000 --warmup 200 --global-warmup 1000`.
The GPU configurations ran `--steps 100000 --warmup 20000 --global-warmup 10000`, apart from M6's three reduced rungs, which kept a warm-up of a fifth and a global warm-up of a tenth of their steps.
Agent configurations set `num_agents` with a square world at the model's default density, as bench_matrix scales it: boids at 141.421 and 1000, ants at 447.214 and 4472.14, gpu_boids at 447.214 and 1414.21, and gpu_ants at 1414.21.

#### Method

Each configuration ran in rounds of A, B, B, A, with rep `i` of each B run paired with rep `i` of the A run beside it through their shared seed.
The ratio is B over A, so a ratio above 1 means B is slower.
A pass began with 120 s idle, and configurations were 30 s apart.
Before every run the harness sampled `top` and waited while other processes held more than 100% of one core in total.
It would have stopped the pass after 15 minutes without a quiet sample, and never did.
The longest waits were for macOS's own daemons: ANECompilerService for 840 s, corespotlightd, mediaanalysisd, contactsd and duetexpertd for 30 s to 210 s each.
Nothing else was built or run during a timing.
The machine was the maintainer's working desktop. The maintainer stepped in to use it once, and pass 2 was stopped for that and resumed from the configuration it lacked, the one in progress discarded.
The cap is about 1000 s of running per configuration.
The wall times below include the waits, which is why pass 2's boids at 50,000 reads 1576 s for about 736 s of running.

#### B0 against the 0.3.0 checkout build

| Configuration | Repetitions × runs | v0.2.0 median | 0.3.0 median | Median ratio | Quartiles | Range | Pairs below 1 | Wall time |
|---|---|---|---|---|---|---|---|---|
| game_of_life 64² | 5 × 10 | 1.511 ms | 1.524 ms | 1.013 | 0.993 to 1.037 | 0.894 to 1.161 | 19 of 50 | 57 s |
| game_of_life 1024² | 5 × 10 | 35.9 ms | 35.6 ms | 1.009 | 0.939 to 1.102 | 0.835 to 1.441 | 25 of 50 | 31 s |
| game_of_life 4096² | 3 × 10 | 200.8 ms | 201.6 ms | 1.003 | 0.982 to 1.044 | 0.920 to 1.100 | 13 of 30 | 47 s |
| sir 64² | 5 × 10 | 1.715 ms | 2.011 ms | **1.069** | 0.978 to 1.238 | 0.682 to 4.642 | 17 of 50 | 26 s |
| sir 64², rerun | 5 × 10 | 1.681 ms | 1.661 ms | 0.992 | 0.830 to 1.062 | 0.652 to 1.501 | 29 of 50 | 57 s |
| sir 1024² | 5 × 10 | 68.7 ms | 70.2 ms | 1.011 | 0.987 to 1.075 | 0.917 to 1.199 | 22 of 50 | 37 s |
| boids 1,000 | 3 × 6 | 437.4 ms | 437.7 ms | 1.003 | 0.988 to 1.014 | 0.931 to 1.355 | 8 of 18 | 40 s |
| boids 50,000 | 3 × 4 | 18.54 s | 18.56 s | 1.013 | 0.982 to 1.037 | 0.898 to 1.069 | 4 of 12 | 903 s |
| ants 10,000 | 3 × 6 | 454.3 ms | 458.5 ms | 1.010 | 1.002 to 1.015 | 0.741 to 1.022 | 4 of 18 | 41 s |
| ants 1,000,000 | 3 × 6 | 12.54 s | 12.67 s | 1.013 | 0.990 to 1.029 | 0.827 to 1.039 | 6 of 18 | 807 s |
| virus_network 10,000 | 5 × 10 | 25.4 ms | 23.7 ms | 0.968 | 0.787 to 1.064 | 0.652 to 1.424 | 31 of 50 | 30 s |
| team_assembly, defaults | 5 × 10 | 0.474 ms | 0.461 ms | 0.979 | 0.941 to 1.028 | 0.816 to 1.216 | 32 of 50 | 26 s |
| gpu_game_of_life 256² | 3 × 6 | 2.77 s | 2.77 s | 0.999 | 0.989 to 1.016 | 0.968 to 1.028 | 10 of 18 | 136 s |
| gpu_game_of_life 4096² | 3 × 6 | 3.74 s | 3.74 s | 1.001 | 0.999 to 1.002 | 0.977 to 1.010 | 7 of 18 | 186 s |
| gpu_sir 1024² | 3 × 4 | 10.65 s | 10.83 s | 0.998 | 0.895 to 1.034 | 0.841 to 1.055 | 7 of 12 | 330 s |
| gpu_boids 10,000, 20,000 steps | 3 × 6 | 7.08 s | 7.08 s | 1.000 | 0.999 to 1.000 | 0.995 to 1.001 | 10 of 18 | 395 s |
| gpu_boids 100,000, 200 steps | 3 × 6 | 1.44 s | 1.44 s | 1.001 | 0.997 to 1.004 | 0.990 to 1.009 | 8 of 18 | 81 s |
| gpu_ants 100,000, 2,000 steps | 3 × 6 | 715.3 ms | 715.2 ms | 1.003 | 0.995 to 1.008 | 0.961 to 1.053 | 7 of 18 | 267 s |

sir at 64² missed at 1.069 and passed at 0.992 on the rerun after cooling.
Its repetitions last under 2 ms, and both runs scatter widely: the first run's pairs range from 0.682 to 4.642, and its quartiles from 0.978 to 1.238.
Every earlier gate found the same noise on this rung (records #30 and #35).
The GPU half lies within 0.998 to 1.003.

#### B0 against the installed form

| Configuration | Repetitions × runs | v0.2.0 median | Installed median | Median ratio | Quartiles | Range | Pairs below 1 | Wall time |
|---|---|---|---|---|---|---|---|---|
| game_of_life 64² | 5 × 10 | 1.516 ms | 1.526 ms | 1.000 | 0.968 to 1.018 | 0.879 to 1.113 | 25 of 50 | 57 s |
| game_of_life 1024² | 5 × 10 | 36.8 ms | 41.0 ms | 1.036 | 0.945 to 1.145 | 0.169 to 1.390 | 22 of 50 | 32 s |
| game_of_life 4096² | 3 × 10 | 205.0 ms | 203.5 ms | 1.003 | 0.985 to 1.029 | 0.919 to 2.221 | 14 of 30 | 48 s |
| sir 64² | 5 × 10 | 1.711 ms | 1.753 ms | 1.003 | 0.922 to 1.140 | 0.663 to 1.557 | 24 of 50 | 57 s |
| sir 1024² | 5 × 10 | 69.2 ms | 70.4 ms | 1.017 | 0.959 to 1.098 | 0.809 to 1.562 | 24 of 50 | 37 s |
| boids 1,000 | 3 × 6 | 438.0 ms | 441.6 ms | 1.004 | 0.991 to 1.027 | 0.961 to 1.432 | 7 of 18 | 40 s |
| boids 50,000 | 3 × 4 | 17.98 s | 18.38 s | 1.040 | 1.009 to 1.066 | 0.995 to 1.088 | 2 of 12 | 1576 s |
| ants 10,000 | 3 × 6 | 453.1 ms | 459.4 ms | 1.013 | 1.002 to 1.024 | 0.992 to 1.072 | 2 of 18 | 72 s |
| ants 1,000,000 | 3 × 6 | 12.49 s | 12.84 s | 1.029 | 1.005 to 1.041 | 0.979 to 1.110 | 3 of 18 | 716 s |
| virus_network 10,000 | 5 × 10 | 29.6 ms | 29.2 ms | 1.003 | 0.893 to 1.084 | 0.743 to 5.839 | 25 of 50 | 31 s |
| team_assembly, defaults | 5 × 10 | 0.483 ms | 0.471 ms | 0.971 | 0.928 to 1.016 | 0.851 to 1.250 | 33 of 50 | 26 s |
| gpu_game_of_life 256² | 3 × 6 | 2.76 s | 2.77 s | 0.999 | 0.991 to 1.005 | 0.983 to 1.013 | 10 of 18 | 136 s |
| gpu_game_of_life 4096² | 3 × 6 | 3.73 s | 3.73 s | 0.999 | 0.998 to 1.001 | 0.986 to 1.007 | 11 of 18 | 186 s |
| gpu_sir 1024² | 3 × 4 | 9.59 s | 9.52 s | 1.002 | 0.989 to 1.014 | 0.953 to 1.043 | 6 of 12 | 361 s |
| gpu_boids 10,000, 20,000 steps | 3 × 6 | 7.11 s | 7.12 s | 1.005 | 0.999 to 1.009 | 0.993 to 1.021 | 5 of 18 | 335 s |
| gpu_boids 100,000, 200 steps | 3 × 6 | 1.44 s | 1.45 s | 0.998 | 0.993 to 1.001 | 0.991 to 1.013 | 12 of 18 | 81 s |
| gpu_ants 100,000, 2,000 steps | 3 × 6 | 734.0 ms | 736.0 ms | 1.005 | 0.998 to 1.007 | 0.991 to 1.081 | 6 of 18 | 112 s |

Every configuration passes on its first run.
Two sit closer to the bound than in pass 1 and with quartiles above 1: boids at 50,000 at 1.040 and ants at 1,000,000 at 1.029.
The installed binary is the same code as the checkout build, at the same opt-level and on the same dependency versions, and differs in where its sources sit, which can move the codegen partitions as record #42 found.
Neither number is a finding of its own, since boids at 50,000 has 12 pairs, and pass 2's B0 median there is 0.56 s off pass 1's.
A cost of a few percent on the large agent rungs of the installed form cannot be ruled out from these numbers.
The ranges hold isolated outliers, 0.169 and 2.221 among them, from single repetitions that a daemon starting mid-run would explain. The medians are robust to them.

#### The web check

`scripts/build_web.sh serve --release` (Trunk's `release` profile) was opened in Chrome as `http://127.0.0.1:8765/?threads=8#dev`.
The console logged "thread pool: 8 workers, 14 reported by the browser" from `henad_app::web` and no "thread pool init failed", `window.crossOriginIsolated` was `true`, and the loading element was gone.
The System tab's Worker threads row was not read.
The tab opened behind the maintainer's working tab, where Chrome neither presents a WebGPU canvas to a capture nor runs `requestAnimationFrame`, and the session left the maintainer's window alone.
The log line comes from the same pool the row reads, as record #38 found when it read both.

### codegen-units

The CPU half of G, the default 16 units as A against `codegen-units = 1` as B, both at opt-level 2 and from the same checkout, by the same method.
Each configuration that missed was rerun once after cooling.

| Configuration | Repetitions × runs | 16 units median | 1 unit median | Median ratio | Quartiles | Range | Pairs below 1 | Wall time |
|---|---|---|---|---|---|---|---|---|
| game_of_life 64² | 5 × 10 | 1.534 ms | 1.616 ms | **1.052** | 1.022 to 1.075 | 0.972 to 1.192 | 5 of 50 | 26 s |
| game_of_life 64², rerun | 5 × 10 | 1.568 ms | 1.659 ms | **1.064** | 1.014 to 1.116 | 0.909 to 1.231 | 10 of 50 | 89 s |
| game_of_life 1024² | 5 × 10 | 38.9 ms | 39.7 ms | 1.006 | 0.920 to 1.155 | 0.782 to 1.371 | 25 of 50 | 32 s |
| game_of_life 4096² | 3 × 10 | 219.9 ms | 227.4 ms | **1.071** | 0.992 to 1.113 | 0.895 to 1.449 | 8 of 30 | 81 s |
| game_of_life 4096², rerun | 3 × 10 | 203.7 ms | 212.9 ms | 1.023 | 0.991 to 1.075 | 0.904 to 1.162 | 10 of 30 | 80 s |
| sir 64² | 5 × 10 | 1.974 ms | 2.078 ms | **1.134** | 0.985 to 1.227 | 0.732 to 1.314 | 14 of 50 | 57 s |
| sir 64², rerun | 5 × 10 | 1.713 ms | 2.014 ms | **1.170** | 1.025 to 1.230 | 0.798 to 1.711 | 7 of 50 | 26 s |
| sir 1024² | 5 × 10 | 83.0 ms | 91.6 ms | **1.103** | 1.077 to 1.141 | 0.968 to 1.363 | 2 of 50 | 71 s |
| sir 1024², rerun | 5 × 10 | 76.1 ms | 85.3 ms | **1.107** | 1.072 to 1.147 | 0.951 to 1.325 | 3 of 50 | 39 s |
| boids 1,000 | 3 × 6 | 446.2 ms | 452.0 ms | 1.011 | 1.007 to 1.018 | 0.940 to 1.031 | 2 of 18 | 41 s |
| boids 50,000 | 3 × 4 | 19.78 s | 18.65 s | 0.967 | 0.937 to 1.008 | 0.869 to 1.021 | 7 of 12 | 833 s |
| ants 10,000 | 3 × 6 | 458.5 ms | 459.4 ms | 1.002 | 0.997 to 1.010 | 0.987 to 1.023 | 7 of 18 | 72 s |
| ants 1,000,000 | 3 × 6 | 12.67 s | 12.50 s | 0.987 | 0.982 to 0.998 | 0.957 to 1.017 | 15 of 18 | 707 s |
| virus_network 10,000 | 5 × 10 | 25.4 ms | 25.2 ms | **1.054** | 0.877 to 1.192 | 0.597 to 1.578 | 23 of 50 | 30 s |
| virus_network 10,000, rerun | 5 × 10 | 30.0 ms | 30.5 ms | 1.003 | 0.928 to 1.070 | 0.671 to 1.475 | 23 of 50 | 94 s |
| team_assembly, defaults | 5 × 10 | 0.457 ms | 0.454 ms | 0.988 | 0.965 to 1.028 | 0.829 to 1.098 | 31 of 50 | 183 s |

One codegen unit is slower beyond the bound on three configurations, on both runs: Game of Life at 64² by about 6%, and SIR at 64² and 1024² by 13% to 17% and about 10%.
SIR at 1024² is the clearest, with 2 and 3 pairs of 50 below 1.
The adoption rule fails, so neither release profile changes, and both keep Cargo's default of 16 units.
The other gains are small and not findings: boids at 50,000 read 0.967 with a 16-unit median 1.2 s above its value in passes 1 and 2, and ants at 1,000,000 read 0.987.

The build-time half of the question was dropped.
The maintainer ruled clean release build times out of scope part way through, after the first build, and none is reported.

### The template check of M10d

Rerun under the setting left in place, the default 16 units, as record #42 ran it.
A is the tutorial's `life`, built in the workspace as a temporary example of henad-tutorial over `henad::cli::run` with `--features henad/cli`, removed after the build.
B is the same `life.rs` copied into a template copy outside any git tree, registered in its `models()`, built against the unpacked tarballs.
B was built twice: on the lock a user's first build writes (`cargo generate-lockfile`), and on a lock seeded from the workspace's `Cargo.lock`.
The seeded lock matched the workspace's in all 399 shared packages. The fresh one differed in 162 of 396, rayon 1.12.0 against 1.11.0 among them.
`--export-stats --seed 1 --steps 300` at 64² and 1024² wrote identical bytes from A and from both B.
Each configuration ran `--json --seed 1 --steps 1000 --warmup 200 --reps 5` in ten rounds, 100 pairs.

| B | Configuration | Workspace median | Template median | Median ratio | Quartiles | Range | Pairs below 1 |
|---|---|---|---|---|---|---|---|
| Workspace's lock | life 64² | 1.593 ms | 1.714 ms | **1.068** | 1.040 to 1.101 | 0.894 to 1.214 | 10 of 100 |
| Workspace's lock, rerun | life 64² | 1.532 ms | 1.669 ms | **1.075** | 1.040 to 1.118 | 0.777 to 1.771 | 9 of 100 |
| Workspace's lock | life 1024² | 45.4 ms | 46.6 ms | 1.011 | 0.954 to 1.090 | 0.609 to 1.506 | 42 of 100 |
| Fresh lock | life 64² | 1.549 ms | 1.522 ms | 0.993 | 0.955 to 1.027 | 0.866 to 1.174 | 57 of 100 |
| Fresh lock | life 1024² | 43.3 ms | 45.3 ms | 1.048 | 0.927 to 1.184 | 0.658 to 1.587 | 38 of 100 |

The result repeats record #42's: a miss at 64² on the seeded lock, on the first run and on the rerun, and a pass on the fresh lock a user gets.
Record #42 traced the miss to the codegen partition the instantiated kernel lands in at 16 units, which one unit fixed in that check.
This session's measurement shows one unit costs more on the engine's own grid models than the partition costs the template, so neither fix is taken.

### Edited tree

```text
.
├── AGENTS.md                                       ~ the template's requirements read "0.3", the retry
├── CHANGELOG.md                                    ~ [0.3.0] undated, a fresh [Unreleased], the links
├── Cargo.toml                                      ~ 0.3.0, and every henad-* requirement
├── Cargo.lock                                      ~ the nine workspace packages at 0.3.0
├── zensical.toml                                   ~ nav entry #44
├── crates/henad-explore/src/tests/kit.rs           ~ the shared accumulator's check retries up to five times
├── templates/model-project/Cargo.toml              ~ henad, henad-build and the dev henad at "0.3"
└── docs/
    ├── license.html                                ~ regenerated, the eight crates at 0.3.0
    ├── guide/your-project.md                       ~ the rename statement
    ├── guide/first-model/gpu-game-of-life.md       ~ "example port's own"
    └── developing/agent-record/20261002-44-library-release.md   +
```

## State after

Everything is uncommitted on `48-library`, on top of 7faf864, in the main checkout as asked.
Nothing is staged, and nothing was tagged, pushed, merged or published.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 1067 tests, none failed, with the wasm32 typechecks, packaging, cargo-deny, the docs and the web build.
  The count equals M11a's.
  The first run failed one test, `a_shared_accumulator_fails_the_thread_count_check`, and the second passed whole, as below.
- `uv run --locked zensical build` passes with no issues, this record and its nav entry included.
- `cargo publish --workspace --dry-run --locked --allow-dirty` in a fresh `CARGO_TARGET_DIR` packages and verifies all eight crates and aborts each upload.
- `scripts/check_packaging.sh` holds every requirement at 0.3.0, and `templates/model-project/Cargo.lock` does not exist.
- The temporary `life_cli` example of the template check is removed, and `examples/tutorial/examples/` does not exist.
- No server is left running. The web build's Trunk server was stopped after the check.

Proposed commit: `feat: release 0.3.0`.

## Issues found & future directions

- **The maintainer's steps remain.** Date `## [0.3.0]` at the tag, merge `48-library` into `master` (Q2), copy the checklist of `docs/developing/releasing.md` into a "Release 0.3.0" issue and run it on `master`, the two verified packaging passes and the `downstream` job among them, then tag `v0.3.0`, publish in the paced order, and publish the GitHub draft.
- **The template's 64² miss stays open.** It holds on a template build seeded from the workspace's lock and not on the fresh lock a user gets, and one codegen unit, which fixed it in record #42, costs the engine's own grid models 6% to 17%.
  The third course of record #42, `#[inline]` on the engine's functions around the row loop, is untested.
  It would need G's CPU half and the template check again.
- **The installed form's large agent rungs** read 1.040 (boids at 50,000) and 1.029 (ants at 1,000,000), inside the bound with quartiles above 1.
  A rerun with more rounds would say whether the installed build pays a few percent there.
- **The Worker threads row of the web check was not read**, as above. The log line and `crossOriginIsolated` were.
  Reading it needs the Henad tab in front for a moment.
- **`a_shared_accumulator_fails_the_thread_count_check` is flaky under load.** In the first `check.sh` run the kit reported `SeedSensitivity` for the broken model and no `ThreadCount` failure.
  The model's result depends on the order its chunks take a shared lock, and under the workspace's parallel test load the seven-worker pool happened to take them in the single thread's order.
  The test passed ten of ten times alone, and in the second full run.
  It predates this session (record #39).
  After the review it retries up to five times, and fails only when `ThreadCount` passes the model on all five.
  `a_build_that_reads_the_pool_width_fails_the_thread_count_check` pins `ThreadCount` deterministically, and no check can catch a lock-order race on every run.
  henad-explore's whole suite then passed three times in a row under its own parallel load.
- **No clean build times** were measured for one codegen unit, by the maintainer's call.
- **Record #43's "pages say 0.3 ahead of the crates"** is resolved by the bump.
- **Next.** The release, then #49 and the rest of v0.4.0.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
