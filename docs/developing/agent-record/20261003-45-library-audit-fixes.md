---
date: 2026-10-03
title: "Library audit: fixes and refutations"
description: An external static audit of the 48-library branch reported 109 findings. Each was re-derived from the code. 94 were fixed in whole or in part and 15 refuted, and a review of the fixes corrected 17 details before the whole check passed.
icon: material/clipboard-check-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: every audit finding fixed or refuted, HENAD_REQUIRE_GPU=1 ./check.sh passing, uncommitted on top of the 0.3.0 bump
baseline_commit: 7faf864
delta_state: uncommitted on `48-library`, mixed with record #44's uncommitted release bump
---

# Library audit: fixes and refutations

> Another model audited the `48-library` branch statically and reported 1 critical, 5 major, 56 minor and 47 nit findings, 109 in all.
> Eight triage agents re-derived each finding from the code, 88 held as reported, 6 held in part, and 15 were refuted with the reason below.
> Eight fix agents applied the 94, five reviewers read the resulting diff adversarially, and one more agent corrected the 17 details they raised.
> The critical finding was real and shipped in 0.2.0: `gpu_ants` ignored its Evaporation parameter, and the tutorial's GPU foraging port taught the same call.
> Ten user-visible fixes join a new `### Fixed` section of the 0.3.0 changelog, and `HENAD_REQUIRE_GPU=1 ./check.sh` passes with 26 more tests than before.

## State before

The release bump of record #44 sat uncommitted on `48-library`, on top of 7faf864, with the shared accumulator's retry in `kit.rs`.
The audit (`dev-docs/deepseek-v4-1-48audit/`, gitignored) reviewed that working tree statically, with no build, test or GPU run.
Its register lists each finding by an `R-NNN` id and an area id, and its verifiers had re-derived every critical, major and minor finding.

## What was done

### Triage

Each finding was re-derived against the working tree, with git history, targeted cargo runs and `gh api` where those settled it.
A finding counts as introduced on the branch only when `git show 773a7a5` shows the code differed at v0.2.0, and a style nit on a comment that predates the rules was refuted, since AGENTS.md leaves those alone.
No finding needed a decision from the maintainer.

### The six critical and major findings

- **R-001, Evaporation on the GPU.** The merge pass of `gpu_ants` and of the tutorial's `gpu_foraging` read the field's parameter from the whole composed list, where index 0 is `num_agents`, so `extract_f32` fell back to 0.999.
  Both now take the field's half of `split_params`, as the CPU engine does, and the page teaching it says so.
  A device-free test reads the merge uniform at 0.95, and the tutorial parity test builds both sides at a non-default value, where both copies had matched each other's mistake at the defaults.
- **R-002, Windows paths in generated code.** The binding assertion's message is written through `Debug` with forward slashes, so a nested shader no longer puts `\s` or `\d` into a Rust literal.
- **R-003, the stamp tests' `init`.** It fell back to itself on a git before 2.45 and recursed until the test binary aborted.
  It now falls back to a plain `git init`, and `committed_crate` goes through it (R-011), so the packed-refs test gets the ref format it asserts on.
- **R-004, `gpu_boids`' declared memory.** `GpuAgentState::demand` sized the index tables to the world's cells, about 278 times the hash grid at the defaults.
  It now reads the `HashGrid` the engine builds, and a registry test checks visual ranges 50 and 1.
- **R-005, the `package` job.** It names `contents: read` and `pull-requests: read`, which `dorny/paths-filter` needs under the repository's read-only default token.
- **R-006, a PSE axis of no cells.** `ResultSet` refuses a folder whose manifest records a search `SearchSpec::check` refuses, through a new `ResultSetError::Search`.
  Patching the subtraction alone would have left the Search view indexing an empty grid.

### The minor findings and nits

The rest are smaller and grouped here by kind.

- **Robustness.** The reduce leaf sizes its partial sums to the folded dispatch (R-015, through a `leaf_blocks` helper the test pins).
  A plan caps its configs over all its blocks (R-056).
  `acquire_headless` checks the baseline before `raise` clamps it (R-025, through `device_limits`).
  A search resume refuses rows past its budget (R-027).
  Save results refuses a link of a results file's name (R-055).
  The template's `seed_buffers` tolerates a short list (R-042).
  The sweep draft ignores an undeclared action index in place of dropping a row (R-034).
  Build is disabled with no model selected (R-035).
- **Provenance and resume.** A resume credits the runs of a session that ended before replacing the manifest (R-023), a merge records its own build as `engine` (R-024), and a changed model reads as `SchemaChanged` (R-028).
- **henad-build.** A `#define_import_path` in a block comment no longer hides an entry (R-007), `@binding (N)` with a space fails the build like any other odd form (R-009), and a path whose `ShaderEntry` variant is no identifier is refused (R-062).
- **Debug.** `agent_lanes!` gives its three types manual `Debug` impls (R-014, R-101), and `Nodes` and `NodeCtx` implement it for any model (R-064), which a test-only model without `Debug` pins.
- **Tests.** `gpu_ants` joins `interleaved_gpu_runs_match_sequential_ones` (R-030), the paused-sweep test runs two lanes (R-029), the GPU handle test compares every file (R-061), the parity comparison fails on NaN (R-016), and the limit tests fail without an adapter under `HENAD_REQUIRE_GPU` (R-060).
  Device-free tests now compare each grid port's seeded buffer with its CPU `init` cell by cell (R-017).
- **Testing kit.** A `ThreadCount` skipped at an overridden size says so (R-020, `SkipReason::OneJobAtOverride`), and the grid job search bisects downward (R-022).
- **Facade.** Eleven more types reachable from listed items, and the `design_csv` module, have `henad::` paths (R-037, R-038).
  The complete example clears its temporary folder first, so a second run succeeds (R-039).
- **Repository.** `check_packaging.sh` holds the template's requirements to the workspace's major and minor (R-051) and reads every include form (R-058).
  The template checks the Mesa tarball's SHA-256 (R-057).
  Four CI steps set `pipefail` (R-050).
  The docs project moves to 0.3.0 (R-052), and the release procedure now names it.
- **Docs and comments.** The rest correct a claim against the code: the docs' GPU seeding sentences (R-017), the settings folder per platform (R-043), the Life page's final `step_cell` (R-040), `rayon` as a dependency of the pool pattern (R-044), the `ok_or` messages (R-045), boids' query buffer (R-046), several CLI reference examples, and the doc lines over 120 columns or with British spelling in a literal.
- **Records.** Record #44 lists the retry in `kit.rs` and `AGENTS.md` and no longer says no workspace code changed (R-048).
  Record #41's grep sentence names the two hits it missed (R-049).

### Refuted

- **R-008**: the shader walk follows a directory symlink, as rustc and Cargo do.
  Skipping it would drop a linked shader directory without a word.
- **R-010**: a package hashes `Cargo.toml.orig` by design, which keeps a package's hash equal to the checkout's, and editing a registry download's generated manifest is no supported workflow.
- **R-026**: a run's series is flushed before its row, so a cut-off write cannot leave a row without its series.
  Only hand damage can, and a check would need its own row count per run kind.
- **R-059**: the per-pass view `Vec` holds one entry per chunk, is documented in place, and any replacement needs `unsafe` or nested zips.
  No measurement suggests a win.
- **R-068**: the comment describes the check `every_example_model_joins_the_set` makes.
- **R-072**: `SweepPreparation` is crate-private, and every caller routes a search spec away first.
- **R-077**: the two `DefaultSetup` comparisons pin engine contracts carried over from a registry test.
- **R-081, R-082**: both CLI messages changed on purpose, and the CHANGELOG and the CLI reference record them.
- **R-085**: the proposed count would use the loop's own filter, and the golden `--list` test already catches a lost model.
- **R-091**: a one-row "One at a time" draft writes the same spec as "Every combination", and reading it back as the latter is correct.
- **R-094**: the About row follows the documented format, and the host's Sources row already gives the hash.
- **R-100**: the releasing page's `v0.3.0` against `0.2.0` is an example of a mismatched pair, and still a correct one.
- **R-106**: the line moved verbatim from v0.2.0's `main.rs`.
- **R-109**: the Build check clones a handful of values once per UI frame.

Of the partial ones, R-018 and R-054 became doc notes, since a resume using the host's execution settings is the existing rule and a browser cannot wait on a paused sweep.
R-045, R-057, R-058 and R-102 fixed the part that held.

### Review of the fixes

Five reviewers read the audit-only diff, the worktree against HEAD with record #44's patch applied.
They found no wrong fix and 17 details: doc claims broader than the code (the source hash, the pool entry rule, `NativeOnly`, the wasm pause, the credited runs), three tests that would pass without their fix (R-015, R-025, R-064), one `Display` text, a comment, two markdown lines, one template call rustfmt splits, and one changelog threshold.
All 17 were applied.

### Edited tree

```text
.
├── AGENTS.md                                   ~ henad-build's deps and binding rule, GPU seeding, the kit's skips
├── CHANGELOG.md                                ~ [0.3.0] ### Fixed, ResultSetError::Search, the settings-folder line
├── pyproject.toml, uv.lock                     ~ henad-docs at 0.3.0
├── zensical.toml                               ~ nav entry #45
├── .github/workflows/ci.yml, release.yml       ~ the package job's permissions, pipefail
├── scripts/check_packaging.sh                  ~ the template's requirements, every include form
├── crates/
│   ├── henad-build/src/                        ~ the escaped message, block comments, binding forms, variant names
│   │   └── tests/                              ~ generate, paths, binding_lines and the stamp init
│   ├── henad-core/src/                         ~ build_info! through __macro_support, Nodes Debug, the plan cap
│   ├── henad-compute/src/                      ~ demand's index grid, leaf_blocks, lane Debug, limit and parity tests
│   ├── henad-models/src/                       ~ Evaporation, the seed tests, the index demand test
│   ├── henad-explore/src/                      ~ resume, merge, result set, device, testing kit, docs
│   │   └── tests/                              ~ gpu_ants tracks, the paused sweep, every file, the new refusals
│   ├── henad-cli/                              ~ doc and test-doc corrections
│   ├── henad-app/                              ~ Build with no model, save links, draft actions, labels, About
│   └── henad/                                  ~ eleven facade paths, the example's message and fresh folder
├── examples/tutorial/                          ~ gpu_foraging's Evaporation, the parity test at 0.95
├── templates/model-project/                    ~ seed_buffers, the Mesa digest, README, models() doc
└── docs/
    ├── authoring/, guide/, reference/          ~ the corrected claims
    └── developing/
        ├── contributing.md, releasing.md       ~ the jobs check.sh leaves out, the docs project version
        └── agent-record/
            ├── 20261002-41-library-tutorial.md ~ the grep sentence
            ├── 20261002-44-library-release.md  ~ kit.rs and AGENTS.md in its tree
            └── 20261003-45-library-audit-fixes.md   +
```

## State after

Everything is uncommitted on `48-library`, in the main checkout, mixed with record #44's release bump.
A patch of the bump as it stood before this session, `m11b-before-audit.patch` beside the audit, lets the two be split for separate commits.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passed after the fixes with 1092 tests, none failed, and after the review corrections with 1093.
- `uv run --locked zensical build` passes with no issues, this record and its nav entry included.
- No separate target directory was left behind.

Proposed commit: `fix: audit findings on the library branch`.

## Issues found & future directions

- **Windows is the only proof of R-002.** On Unix the new test passes with or without the fix, and the `windows-2025` leg of CI is the first build of a nested shader there.
- **R-005 is unproven in CI.** The `package` job has never run, and the 403 the audit predicts was not reproduced.
- **R-025's test reaches the helper alone.** An edit that bypasses `device_limits` in `acquire_headless` would go unnoticed without a device.
- **The audit's questions for the author** are all answered by the fixes or refutations above, the resume's execution settings (R-018) and the wasm pause (R-054) among them as documented behaviour.
- **Next.** The maintainer's release steps of record #44.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
