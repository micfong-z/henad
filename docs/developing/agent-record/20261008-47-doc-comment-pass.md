---
date: 2026-10-08
title: "Doc comment pass"
description: A comments-only pass documented every public item of the eight published crates, trimmed the existing comments to the house style, and then rewrote comments and docs prose in plain technical English. Every published crate now has 100% rustdoc coverage and warns on a missing doc, and reviewers caught 70 comments that stated something false.
icon: material/comment-text-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
state: every public item documented, missing_docs warned in the eight published crates, comments and docs prose in plain technical English, HENAD_REQUIRE_GPU=1 ./check.sh passing, uncommitted on top of 6a1f762
baseline_commit: 6a1f762
delta_state: "uncommitted on `master`, 349 changed files and this record"
---

# Doc comment pass

> The maintainer asked for a doc comment on every public item of the published crates, and for the existing comments to be trimmed to the house style, with no code changes.
> Eleven agents each edited a separate part of the workspace, and a separate reviewer for each part checked every changed comment against the code.
> A second, smaller round documented the fields of one-line enum variants, which rustfmt splits over several lines with a trailing comma.
> Every published crate now has 100% rustdoc coverage, up from between 41.5% and 75% in six of them, and `missing_docs` reports nothing on native or wasm32.
> After reading the result, the maintainer asked for plain technical English instead of strict grammar, and two more rounds rewrote the comments and the docs prose to match.
> Last, each published crate gained `#![warn(missing_docs)]`, and one read of every comment in the workspace made about 380 more fixes.

## State before

`master` was at `6a1f762`, the merge of #50, with a clean tree.
Coverage, from `cargo +nightly-2026-09-30 rustdoc -p <crate> --all-features --lib -- -Z unstable-options --show-coverage`:

| Crate | Documented | Coverage |
|---|---|---|
| henad-core | 846 | 62.5% |
| henad-build | 25 | 51.0% |
| henad-compute | 360 | 65.6% |
| henad-models | 59 | 41.5% |
| henad-explore | 790 | 73.9% |
| henad-cli | 7 | 100.0% |
| henad-app | 12 | 75.0% |
| henad | 18 | 100.0% |

About 1,090 public items had no doc.
Early milestones had also left terse fragments, module docs of up to 84 lines, and design narration, history and benchmark figures in docstrings.

## What was done

### Tools

Every agent used three commands.

- The coverage command above, for the totals.
- `cargo +nightly-2026-09-30 rustc -p <crate> --lib --all-features --profile check -- -W missing_docs`, for the exact items and their lines.
- `comment_only.py`, a script written for this session and kept outside the repository.
  It lexes the HEAD and working-tree versions of every changed `.rs` and `.wgsl` file, drops the comments, and fails when the remaining tokens differ.
  It handles strings, raw strings, char literals and lifetimes, and it caught a planted rename.
  From the second round on, it also ignores a comma right before a closing brace.

### Round one: coverage and trimming

Eleven units, each with one editing agent followed by one reviewer, ran as a pipeline from 17:18 to 17:49.

| Unit | Files |
|---|---|
| core-authoring | henad-core except `explore/` and `export/`, including the README and the shared WGSL |
| core-explore | `explore/` and `export/` in henad-core |
| build | henad-build |
| compute | henad-compute |
| models | henad-models, except the `gpu_game_of_life` and `gpu_ants` shaders |
| explore-core | henad-explore except `output/`, `testing/` and `src/tests/` |
| explore-io | `output/`, `testing/` and `src/tests/` in henad-explore |
| cli-facade | henad-cli and henad |
| app-sweep | `ui/sweep/` in henad-app |
| app-rest | the rest of henad-app, except the two generated icon tables |
| tutorial-template | `examples/tutorial`, the template, the guide pages that include them, and the two shipped shader sets that the tutorial copies byte for byte |

Each editor documented the missing public items in its files and trimmed the existing comments.
Comments already in style stayed unchanged, so every edit fixes a specific problem.
Agents did not edit docs pages outside their unit or any file under `docs/developing/`, and reported narration worth keeping instead.
Every unit kept the `--8<--` markers and changed a comment inside a region only when the page still read correctly.

Each reviewer read its unit's diff hunk by hunk and checked every changed comment against the code it describes and its callers.
The eleven reviewers made 86 fixes.
47 corrected false statements, all but one introduced by the pass, and 2 restored contracts lost in a cut.
Some of the false statements:

- The model trait docs referred to a "Model metadata panel" that the app does not have.
- `PUBLISH_INTERVAL` and `SNAPSHOT_INTERVAL` were described as the shortest time between any two publishes. A pause, a step or an action publishes immediately.
- `AppOpening` was said to replace the first model, which is false for `AppOpening::Results`.
- The reduce pass of the GPU Game of Life was said to run at the display cadence, which is true only in the app.
- `RNG_INIT_SEED` was called the `rng` buffer seed. A seeded run mixes it in as a domain separator.

The editors also corrected comments that were already wrong at HEAD:

- `reduce_tree.wgsl` said only invocation 0 gets the total. Every invocation reads it.
- `Grid2D` was described as wrapping toroidally. It does no wrapping.
- `buffer_lens` and `seed_buffers` said "in binding order". They follow `BUFFERS`.
- The boids stat palette comments read "Max speed" and "Min speed". The stats are average speed and average velocity, and the first entry is unused.
- An ants comment referred to a gap report that is not in the repository.

The two restored contracts were a guard at the debug assertion in `GpuTrack` that AGENTS.md describes, and the caveat that `--export-stats` does not time a run like a benchmark.

### Round two: enum variant fields

After round one, about 190 public fields were still undocumented.
Each was in a one-line enum struct variant, such as `BadEntry { raw: String }`.
A doc on such a field makes rustfmt split the variant over several lines and add a trailing comma, and the checker rejected that comma.
A comma right before a closing brace never changes a program, so the checker now ignores it on both sides.
Four agents documented 249 fields from 17:51 to 18:15:

- 102 fields of the error and spec enums in henad-core;
- 101 fields of the error enums in henad-explore;
- 26 fields in henad-compute, henad-build and henad-app;
- 20 lane fields of `BoidLanes`, `VirusLanes` and `TeamLanes`, inside the `lanes` regions that the authoring pages show.

A lane field needs no trailing comma.
`agent_lanes!` moves a doc on a lane onto its field, and onto both fields of a `dual` lane, as `docs/authoring/agent-models.md` already says.

The henad-explore reviewer failed on a network error, and the workflow, resumed from 19:07 to 19:13, ran all four reviewers again.
The reviews caught 23 field docs that were imprecise or wrong.
For example, the `min` of a reversed range was called its lower end, and a `row` that counts every record of the text, including blank ones, was said to count from the header.

### Narration moved to the developer pages

Fourteen passages of reasoning removed from the source went into `docs/developing/`, each checked against the code first.

- `cpu-backend.md`: the boids hash cell size, why `run_pass` is an inherent method of the generated lanes, the engine owning the world extent, the measurement behind the 8192-cell floor of the row loop, the boids `CHUNK` of 64, and the crossover between the shadow and banded scatter arms.
- `gpu-backend.md`: why henad-build reads binding lines itself, the readback carrying `f32` sums as words, one frame of staleness, `GpuSimThread` accepting the CPU runner's commands, and a new trap, a per-step seed in a uniform repeating across a batch. The timestamp trap lost its "now".
- `architecture.md`: the three tests that keep the tutorial pages in line with the code, the `SimRunner` enum in the app, one agent renderer for both backends, and parameter text checked against its bounds before a build.

One passage, on why `StatsWriter` is generic over its writer, was dropped.
No section fits it, and its two callers make the reason clear.

### Rounds three and four: plain technical English

The maintainer read the result and found the register unnatural.
The rules that asked for complete sentences and one clause per idea had produced elliptical grammar: clauses ending on a possessive or a quantifier ("This macro forwards to henad-core's.", "or all of them when it asks for none"), "of" chains ("the descriptor of the device"), and home-made verbs ("asks for a single worker").
They asked for compound nouns, named nouns, standard technical verbs and easy grammar, and for every instance to be fixed.
The rule is now in AGENTS.md as "Plain technical English" under Sentences, and "Complete sentences" became "No clipped notes", which allows fragments in the style of standard API docs.

Round three ran from 20:11 to 20:33 over sixteen units: the eleven code units of round one, with the tutorial split into a CPU half and a GPU half, and four docs units for the guide, authoring, reference and developer pages.
Sonnet agents edited and Opus agents reviewed, each reviewer comparing against a snapshot of the tree taken before the round (`git stash create`, which writes an object and leaves the tree, index and refs alone).
The editors changed 1,887 sentences.
The reviewers fixed 1,667 more instances that the editors had missed, restored the meaning of 83 rewrites, and reverted 46 rewordings that only changed taste.

A pattern search afterwards found three more shapes: count fields documented as a bare plural ("Runs of the whole plan.", "Bytes the held series take."), verbless superlatives ("Most samples the Samples field accepts.", "Widest the form's column grows"), and "is no Rust identifier".
Round four fixed them from a line list, with the standard "Number of ..." and "Maximum ..." forms.
Five Sonnet editors changed 252 lines from 20:36 to 20:44.
Five Opus reviewers restored the meaning of 7 of them, fixed the wording of 94 more, and reverted 27 edits to lines that were already natural, such as "Lowest value." and "Largest step of a gene".
The `heap_bytes` docs now share one form, "Heap memory held by the lanes, in bytes.", and the GPU buffers say "Approximate device memory held by ...".

UI strings, error messages and clap's `--help` text are code and stayed unchanged.
So did the dated session records, apart from this one.

### Round five: the lint and a final read

The maintainer asked for the `missing_docs` lint on the published crates, and for one more read of every comment.

Each of the eight published crates has `#![warn(missing_docs)]` in `lib.rs`, right after the `doc_cfg` attribute.
Clippy runs with `-D warnings` in `check.sh` and CI, so an undocumented public item fails the build on native.
The lint leaves out the tutorial, the template and the binaries.
`[workspace.lints]` would also reach the tutorial, and a crate `[lints]` table cannot add a lint next to `workspace = true`.
AGENTS.md describes the lint under Lints.

For the read, a script wrote every comment of the workspace with the line under it into fifteen files, and every file was read in full.
The read made about 380 fixes, applied with a script that rejects an edit whose old text does not occur exactly once.
The fixes fall into a few kinds:

- About 45 error variants ended in "for the reason inside" or "for the reason given", a phrase that added nothing to the error the variant holds.
- A function or a test documented with a bare noun phrase now opens with "Returns" or "Checks".
- Banner comments such as `// --- Setup ---` became plain comments, in the source, the tutorial and its tests.
- Rates written as "64 rows a job" and "2 candidates a batch" now read "per".
- Five `# Panics` headings gained the blank line after them.
- Two ant comments, "No pheromone nearby, so probably keep going the way we were." and "Stronger route wins the cell", were rewritten in the shipped shaders, the tutorial copies and the guide pages together, so `tests/shaders.rs` and `tests/snippets.rs` still pass.
- A few claims were checked against the code and corrected, among them which merge rows are left out (series rows of a run with no `runs.csv` row) and when a directory's lock is released.

### Decisions

- **Trivial accessors got a short noun phrase.** The style allows an accessor to stay undocumented, but the task asked for every public item. `directed`, `version`, `edge_count` and `heap_bytes` in network.rs gained one line each, and the existing comments there are untouched.
- **The tutorial stays at 38.7% coverage.** It is unpublished, each page includes its file whole as everything the reader wrote, and the Game of Life page counts the lines of `life.rs`. Its comments follow the style, and every changed line is mirrored in the page code block that `tests/snippets.rs` compares with it.
- **AGENTS.md changed only in its writing rules and its Lints section.** No agent moved a comment that AGENTS.md refers to. The reserved-name table stays in `binding.rs`, on `RESERVED`, and the module docs of `ui/mcs.rs` and `scatter.rs` still contain what AGENTS.md sends readers there for.
- **Comments in the shared WGSL modules changed**, which changes `SHARED_WGSL_FNV1A64`. Every crate with shaders regenerates its bindings once, and henad-build's `output.rs` triggers the same through `GENERATOR_SOURCE`.

### Module docs shortened most

| File | Lines before | Lines after | Content moved to |
|---|---|---|---|
| `henad-core/src/authoring/model/gpu_grid_model.rs` | 84 | 8 | `authoring/gpu-grid-models.md`, which already had it, and the items each contract applies to |
| `henad-core/src/authoring/model/gpu_agent_model.rs` | 45 | 9 | `authoring/gpu-agent-models.md`, `shaders.md` and the items |
| `henad-compute/src/gpu/sim_thread.rs` | 28 | 7 | `developing/gpu-backend.md`, Batching |
| `henad-core/src/authoring/model/binding.rs` | 28 | 8 | the doc of `RESERVED` |
| `henad-models/src/gpu_game_of_life/mod.rs` | 28 | 9 | the guide page, which has the memory arithmetic |
| `henad-explore/src/output/mod.rs` | 19 | 6 | the doc of `OutputDir::replace_tables` |
| `examples/tutorial/src/lib.rs` | 16 | 8 | `developing/architecture.md` |
| `henad-compute/src/gpu/primitives/readback.rs` | 12 | 5 | `developing/gpu-backend.md`, the primitives |
| `henad-compute/src/simulation.rs` | 16 | 10 | the docs of `Simulation` and `RunSetup::build` |

### Coverage after

| Crate | Before | After |
|---|---|---|
| henad-core | 846 (62.5%) | 1354 (100.0%) |
| henad-build | 25 (51.0%) | 49 (100.0%) |
| henad-compute | 360 (65.6%) | 549 (100.0%) |
| henad-models | 59 (41.5%) | 142 (100.0%) |
| henad-explore | 790 (73.9%) | 1069 (100.0%) |
| henad-cli | 7 (100.0%) | 7 (100.0%) |
| henad-app | 12 (75.0%) | 16 (100.0%) |
| henad | 18 (100.0%) | 18 (100.0%) |

`missing_docs` reports nothing for any of the eight crates on native, or for the six wasm32 crates on wasm32, which include `start_web` and the other web-only items.

### Edited tree

```text
.
├── AGENTS.md                  ~ Plain technical English, No clipped notes, missing_docs under Lints
├── crates/
│   ├── */src/lib.rs           ~ #![warn(missing_docs)] in the eight published crates
│   ├── henad-core/            ~ every public item documented, GPU trait module docs cut to a link
│   ├── henad-build/           ~ ShaderBuildError fields, crate doc tightened
│   ├── henad-compute/         ~ runner and engine docs, guards kept as one line each
│   ├── henad-models/          ~ model types and params, lane fields, stale palette and seed comments corrected
│   ├── henad-explore/         ~ error fields, testing kit items, output/ module doc cut
│   ├── henad-cli/             ~ trimmed, crate doc command lines unchanged
│   ├── henad-app/             ~ AppOpening fields, private UI comments trimmed, UI strings unchanged
│   └── henad/                 ~ trimmed
├── examples/tutorial/         ~ comments in the style
├── templates/model-project/   ~ comments in the style, crate attributes equal to the tutorial's
└── docs/
    ├── guide/, authoring/, reference/   ~ prose in plain technical English
    └── developing/
        ├── architecture.md, cpu-backend.md, gpu-backend.md   ~ narration from the source, plain English
        └── agent-record/
            └── 20261008-47-doc-comment-pass.md   +
```

`zensical.toml` gains the nav entry for this record.

## State after

Everything is uncommitted on `master`, on top of `6a1f762`.

All checks ran on the final tree:

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes with 1203 tests, none failed and 4 ignored, the same count as record #46. That covers fmt, clippy, the wasm32 typechecks, packaging, cargo-deny, the docs with `-D warnings` and the web build.
- `python3 scripts/docs_rs.py --host` builds all 13 docs.rs targets, native and wasm32, with `-D warnings`.
- `uv run --locked zensical build` reports no issues, with this record in the nav.
- `comment_only.py` passes over the whole tree: 297 source files changed, 289 in comments only apart from the trailing commas added by rustfmt, and the eight crate roots, whose one other change is the lint line.
- A structural comparison of the 52 changed Markdown and TOML files against HEAD finds the same headings, include lines, link targets, admonitions, front-matter keys and code. The only exceptions are the comments that mirror the tutorial files and `complete.rs`, which `tests/snippets.rs` and `the_readme_program_matches_the_example` check, and the nav entry in `zensical.toml`.
- `missing_docs` is now a lint. Clippy passes with it under `-D warnings` on native, and the wasm32 typechecks of `check.sh` print no warning.

`target/doc-coverage` and `target/doc-private`, the two scratch target directories this session created, are deleted.

Proposed commit: `docs: document every public item and rewrite comments in plain technical English`.

## Issues found & future directions

- **`missing_docs` warns but does not fail on most of wasm32.** CI's wasm32 clippy covers henad-app and the facade with `-D warnings`. A web-only item of henad-core, henad-compute, henad-models or henad-explore that lacks a doc only prints a warning in the wasm32 typecheck of `check.sh`.
- **Code or AGENTS.md drift, found by a reviewer.** `spec_file.rs` accepts a table path that starts with a `.` component, while AGENTS.md says a path containing `.` is rejected.
- **Left for the maintainer:**
  - `docs/guide/your-project.md` states a measured figure in its prose: a toy kernel at `opt-level` 1 ran 4.7 times slower than at 2.
  - `docs/benchmarks.md` says "Read the column, not the row", and its meaning is unclear next to the table.
  - `docs/authoring/grid-models.md` says a GPU port calls `init` to seed its buffers. The shipped grid ports repeat its draws instead, and only the agent ports and the template's `gpu_vote` call it.
  - `docs/developing/releasing.md` has a future-plan sentence ("stays an option should the step ever be automated").
  - Some clap help lines are clipped ("Ramps GPU clocks and pays first-use compilation, so rep 1 isn't cold.").
  - `supports_compute` says every backend except `Gl` runs compute shaders, but native GL with GLES 3.1 can.
  - `ResultReplayError::Mismatch` lists the config, replicate, seed and run key, and for a search run it also fires when recorded value columns do not match the schema.
  - The crate doc of henad-app and `AppOptions::opening` say an opening replaces the first model, which is imprecise for `AppOpening::Results`.
  - Line 8 of `network.rs`, the maintainer's comment on `Csr`, is 122 columns long.
- **The next patch release.** The WGSL diff in the release checklist will list the comment changes in the shared modules.
- **The checker is not in the repository.** It could move to `scripts/` if a later pass needs the same guarantee.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
