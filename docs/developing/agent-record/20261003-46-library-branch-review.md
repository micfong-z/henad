---
date: 2026-10-03
title: "Library review: the whole branch"
description: An independent multi-agent review of the whole 48-library branch against v0.2.0 found 227 distinct issues, 6 of them major. All were fixed apart from one the agent rules forbid, a review of the fixes found 47 more problems that were fixed in turn, and an interleaved benchmark against de992ee stayed within the 1.05 bound.
icon: material/file-search-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: every finding of the branch review fixed or decided, HENAD_REQUIRE_GPU=1 ./check.sh passing with 1203 tests, uncommitted on top of de992ee
baseline_commit: de992ee
delta_state: "uncommitted on `48-library`, 223 changed and 14 new files"
---

# Library review: the whole branch

> The maintainer asked for an independent review of everything `48-library` changes since v0.2.0, before the merge and the first publish.
> 36 reviewers covered the branch by area and by cross-cutting lens, adversarial verifiers checked every finding, and a completeness critic sent 8 more reviewers to the angles nobody had taken, the live app, the web build and a release rehearsal among them.
> After merging duplicates, 227 issues remained: 6 major, 93 minor and 128 nit.
> Nine fix agents, a rename pass and an integration pass fixed all but one, which sits in the maintainers' sections of the records.
> Six reviewers then read the fix diff and found 47 problems, 11 of them bugs, two in the new results-folder lock, and four agents fixed those.
> `HENAD_REQUIRE_GPU=1 ./check.sh` passes with 1203 tests, 110 more than before, and an interleaved release benchmark against `de992ee` stays within the 1.05 bound on all seven configurations.

## State before

`48-library` stood at `de992ee`, with the 0.3.0 bump (`4ae88b5`) and the external audit's fixes (record #45) committed and the tree clean.
The branch was not pushed.

## What was done

### The review

The review ran in the background from 12:36 to 14:32.
The reviewers were told not to read the earlier audit, to keep the review independent.

- **Round one.** 28 area reviewers read every changed file in full, and 8 lenses read across the branch: the public API before publishing, the dependency graph and features, determinism, hot-path performance, panics in the library API, documented commands executed against the binary, security, and test coverage.
- **Verification.** Every critical or major finding went to three verifiers, one each for reproduction, intent and impact, and stood on a majority. Every minor finding went to one verifier, and each area's nits to one batch verifier. 12 of 295 raw findings were refuted.
- **Round two.** A completeness critic named 8 uncovered angles:
  - output equality with 0.2.0 for all ten models, and a 0.2.0-written folder read by 0.3;
  - a release rehearsal on the packaged tarballs;
  - the live native app, driven through the egui MCP server;
  - the web app in a browser;
  - the tutorial, executed page by page as a reader would;
  - concurrent library use within one process;
  - platforms CI never runs;
  - the rendered docs site.

  Their findings went through the same verification.
- **Synthesis.** One agent per area merged duplicates, and a final pass merged across areas and checked the result against the earlier audit's refutations.

The report is `dev-docs/branch-review-48/REPORT.md` (gitignored), with every finding, its evidence, the refuted ones and each reviewer's coverage.

The six majors:

- The `downstream` job's model-edit step could never pass. `CARGO_TERM_COLOR=always` put an escape code between "Compiling" and the crate name.
- A PSE search whose axis came from Use range from results could fail to resume. Without `float_roundtrip`, serde_json read 17-digit floats one ULP off.
- A GPU `Simulation`, `--export-stats` and the GPU benchmark never reported a lost device. They went on returning the last stats under new ticks.
- In a browser, Save results reported Saved after Cancel and dropped the unsaved-results guards.
- In a browser, a GPU run-to straight after a rebuild recorded a row of zeros at tick 0.
- A paused single-lane sweep waited inside rayon's global pool. A worker it held could freeze another simulation in the same process.

Four findings reopened refutations or partial fixes of record #45: the shader walk's symlink loops (R-008), a stray `Cargo.toml.orig` (R-010), the app's resume settings (R-018) and the template's unverified Trunk download (R-057).

### The fixes

The findings came in after the maintainer's 13:45 cut-off for automatic fixes. They waited until he asked for all of them to be fixed.

- **Nine fix agents**, one per group of files: build, core and compute, the tutorial and template, explore core, the testing kit, the CLI and facade, the app, CI and release, and the docs.
  Each re-checked its findings, fixed them, added a test that fails without the fix where the finding was behavioural, and wrote its outcomes to `dev-docs/branch-review-48/fix-results/`.
  Of 226 recorded outcomes, 206 were fixed as suggested and 17 differently. The other 3 were changelog edits that the integration pass applied.
- **Two agents stopped at the session limit**, the app and core-compute groups. Both resumed with their context after the limit reset.
- **The renames ran alone afterwards**, since they touched every group's files:
  - `gpu_memory` became `gpu_memory_budget` on both option structs and `Executor`.
  - The constant `buffers!` emits became `BUFFER_SPECS`.
  - `ProgressUpdate`'s `elapsed_s` and `remaining_s` became `Duration`s.

  The serialized names stay unchanged: the TOML key, `--gpu-memory` and the JSON keys.
- **An integration pass** filed the 88 proposed changelog items under Added, Changed or Fixed. It found nine more breaking changes no group had proposed, finished the leftover cross-group items, and made fmt, clippy, docs, packaging, cargo-deny and the site build clean.
- **The finding not fixed** is [230], the seed notes in the records' human sections. AGENTS.md forbids agents to edit them.

### The review of the fixes

Six reviewers read the fix diff by slice and found 47 problems.
The 11 bugs:

- **The new results-folder lock**, two bugs:
  - A resume checked the lock but took it only after scanning the tables.
  - The writer deleted `.lock` while it still held it.
- **Planning limits**, two bugs:
  - A search's batch times its replicates was still unbounded.
  - `MAX_RUNS` at 2^28 still let plans of gigabytes through.
- **Shader imports**, one bug: a quoted import such as `#import "std.inc"` got past the reserved-name check.
- **The app**, five bugs:
  - A multi-file browser download counted as saved.
  - Resume applied a lane count recorded on another machine.
  - The unidentified-build warning vanished beside a changed one.
  - A resumed sweep's runs read as unknown candidates.
  - Huge plans were still built in full.
- **The tests**, one bug: the complete example's test swept into one shared temporary folder, so two concurrent runs could delete each other's output.

Four agents fixed all 47.
The lock is now taken before the scan and the file stays in place, `MAX_RUNS` is 2^24, equal to `MAX_CONFIGS`, and a search's batch is held to it.

### Decisions taken in the fixes

- `FaultKind` is `#[non_exhaustive]` and gains `DeviceLost`. So do `ExploreError`, `SweepOutput` and `ShaderBuildError`.
- A results folder takes the operating system's advisory lock on `.lock`, through `File::try_lock`. A killed process cannot leave the folder locked, and the file stays once a writer ends.
- `henad::action` keeps its singular name. A module `actions` beside the `actions!` macro would repeat the docs.rs collision fixed for `params`.
- `henad::gpu::raise` is `henad::gpu::raise_limits`.
- `henad-cli` refuses a malformed `--set`, `--act`, `--vary`, `--stop` or `--reduce` value with status 2, and `--spec` without `--out`, `--dry-run` or `--params`.
- In a browser a single-file save counts as saved once its download starts. Save results keeps its button and both unsaved-results warnings, since a browser can hold back every download after the first.
- The app's Resume sweep applies the recorded memory budgets and leaves concurrency automatic. `henad-cli --resume` still applies only a spec's table and its own flags.
- A folder whose manifest records more than 1,048,576 configs opens in the app from `runs.csv` alone, without planning, and its runs do not replay there.
- henad-app's licence reads `(MIT OR Apache-2.0) AND OFL-1.1 AND Apache-2.0`, and the official About window shows that expression.
- docs.rs builds henad-app's wasm32 docs without atomics. The thread-pool code exists only with atomics on.
- The stability policy counts the majors of toml, serde_json, rayon, log and wasm-bindgen-rayon as part of the API.
- The template gains:
  - `git init` in its fetch commands;
  - `#![recursion_limit = "256"]`;
  - a SHA-256 for the Trunk tarball beside the version pin;
  - LF line ends for its scripts;
  - a read-only token in its workflow;
  - `env_logger` and `wasm-bindgen-futures` behind its `app` feature.
- A single sweep lane gets a rayon pool of its own at the global width. A paused run then holds a worker of that pool, never one of the global pool another host uses.

### Performance

The fixes touched hot paths:
- `wrap_coord` gained a compare and select, run per agent in Boids and Ants.
- `Network`'s accessors gained `#[inline]`.
- A `u32` tally saturates in its per-chunk merge.

A release `henad-cli` of `de992ee`, built from an archive in its own target directory, ran against the working tree's. Each configuration ran 12 interleaved rounds, alternating which binary went first, with 3 repetitions after one warm-up, on a machine with no agents running.
The ratio is new over old, the median of the rounds.

| Configuration | Median ratio | Quartiles |
|---|---|---|
| boids 50,000 | 0.989 | 0.984–1.034 |
| ants 1,000,000 | 0.999 | 0.993–1.017 |
| virus_network 1,000,000 | 0.979 | 0.972–0.996 |
| virus_network 10,000 | 1.010 | 0.986–1.150 |
| game_of_life 4096² | 1.000 | 0.996–1.009 |
| game_of_life 64² | 0.993 | 0.989–0.995 |
| sir 4096² | 1.023 | 1.012–1.042 |

All seven stay within the 1.05 bound. An earlier pass of 8 rounds read between 0.981 and 1.022.
The machine had worked for hours before the run and was not cooled, and the quartiles of the two smaller configurations stay wide.

### Edited tree

```text
.
├── AGENTS.md, CHANGELOG.md                     ~ the fixes' facts, [0.3.0] Added, Changed, Fixed and Breaking lines
├── Cargo.toml, Cargo.lock, deny.toml           ~ serde_json float_roundtrip, pollster 1.0, cargo-deny over all features
├── check.sh, .gitattributes, zensical.toml     ~ facade wasm32 with all features, LF scripts
├── .github/workflows/ci.yml                    ~ downstream colour, Trunk SHA-256, msrv, docs and features on dispatch
├── scripts/                                    ~ check_packaging rules, docs_rs.py +, docs frontmatter test +
├── crates/
│   ├── henad-build/                            ~ symlink cycles, imports outside the root, git quirks, new errors
│   ├── henad-core/                             ~ ValueError shape, plan limits, wrap_coord, pcg_hash, GPU contracts docs
│   ├── henad-compute/                          ~ DeviceLost, faulted Simulation, contracts.rs +, browser tick 0, Debug
│   ├── henad-models/                           ~ seed and contract tests, gpu_contracts.rs +, wgpu as dev-dependency
│   ├── henad-explore/                          ~ the lock, single-lane pool, resume and merge provenance, kit checks
│   ├── henad-cli/                              ~ exit status 2, about, no device for a CPU-only set, json_lines.rs +
│   ├── henad-app/                              ~ browser downloads, resume budgets, huge folders, licence, docs.rs
│   └── henad/                                  ~ params! forward, explicit helpers, complete.rs test +, raise_limits
├── examples/tutorial/                          ~ snippets.rs +, thread_count.rs +, the reader's crate builds per page
├── templates/model-project/                    ~ .gitattributes +, trunk-sha256 +, web-checks.sh +, git init, features
└── docs/
    ├── guide/, authoring/, reference/          ~ corrected claims, facade paths, platform maths, the lock
    └── developing/
        ├── releasing.md                        ~ the stability policy, checklist items for docs.rs and versions
        └── agent-record/                       ~ quoted frontmatter #30 to #45, #39's correction
            └── 20261003-46-library-branch-review.md   +
```

## State after

Everything is uncommitted on `48-library`, on top of `de992ee`, in the main checkout.
Nothing was pushed, tagged or published.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes with 1203 tests, none failed. That covers the wasm32 typechecks, packaging, cargo-deny, the docs and the web build.
- `uv run --locked zensical build` reports no issues.
- The baseline build and every scratch copy are deleted.

Proposed commit: `fix: findings of the branch review`.

## Issues found & future directions

- **Not verified on CI.** The `downstream` job, the `package` job's permissions and the Windows leg have never run.
  The colour fix, the Trunk checks and the Windows path fixes are first proven there.
- **Maintainer calls left open:**
  - Whether models should route their maths through libm, so a browser and a desktop step to the same bits, or whether replays and merges should compare platforms. Only the docs changed (finding 187).
  - Whether a new variant of a `#[non_exhaustive]` enum can ship in a patch release.
  - Whether the testing kit should fail a grid cell past the palette (131).
  - The seed notes in the human sections (230).
- **The GPU benchmark at `--warmup 0`** still times wgpu's lazy zero-fill of buffer sides the engines leave unseeded. Clearing them at build would close it (73).
- **`henad-cli --resume` ignores the execution settings a folder records**, unlike the app's Resume sweep.
- **A second pollster**, 0.4, stays on Linux through rfd's portal feature.
- **`the_engine_stamp_reads_cargo_vcs_info`** failed twice while other agents edited the crates it packages, and passed alone. It reads the tree twice and is exposed to concurrent edits.
- **A downstream test that turns a GPU state into a trait object** will need `recursion_limit` once the nightly lint becomes an error. The template sets it, and the guides do not mention it.
- **`~/Library/Application Support/Henad` was deleted** by the app fix agent, which said its own test launch created the folder. 0.2.0's `Henad-Engine` folder is untouched.
- **Next.** Commit, push for the draft PR's CI, then the release steps of record #44.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
