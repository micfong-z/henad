---
date: 2026-11-24
title: "Fixes, Windows CI, a docs review and test tiers"
description: "After the fold of record #109, the maintainer asked for an app icon, a new demo, four fixes, PR #52 on the branch, CI passing, a docs consistency check before the merge into master, a pass against AI-like wording, and a faster CI. CI had never run on this branch, and its Windows job found three faults: CRLF checkouts of embedded text, a WARP crash on the probe tallies' barrier, and abs(-0.0) under FXC. Four reviewers fixed about 80 wrong facts in the docs, five agents reworded about 300 sentences, and CI now runs a quick tier on every push and the full suite nightly and on a dispatch."
icon: material/timer-sand
status: ai-generated
model: "Claude Code. Agents made the fixes, the Windows investigation, the docs reviews and the style pass, and the session wrote this record"
issue: "#37"
state: "37-node-editor with the test tiers and this record on top of d3a8c75b, the remote's head of the branch. CI run 45270855758 passed every job at 82279934. Run 45301037979 at d3a8c75b failed one flaky test on Windows, which the pause in the shared accumulator fixes"
baseline_commit: 5b83271a
delta_state: "committed on `37-node-editor`: 21 signed commits pushed up to `d3a8c75b`, then the shared accumulator's pause, the test tiers and this record"
---

# Fixes, Windows CI, a docs review and test tiers

> After record #109 the maintainer asked for help marks as plain labels, an app icon from the Figma logo, a new demo gif and video, four fixes, PR #52 on the branch, CI passing, a docs consistency check before the merge into master, a pass against AI-like wording, and a faster CI.
> The four fixes are a picker that kept focus away from its number box, three unnamed Tab stops, a World Width of 10000 snapping to 10001, and Ant Foraging delivering nothing in large worlds.
> The first three were faults, fixed with tests. The fourth comes from the reference model's constants, and only the docs changed.
> CI runs on `master`, on pull requests and on a dispatch, so this branch had never run it. Its Windows job found three faults that no other platform shows.
> Git checked out embedded WGSL and TOML with CRLF line ends, WARP crashed on the probe tallies' barrier inside an `if`, and FXC returned `-0.0` for `abs(-0.0)`.
> Four reviewers checked every published page against the code and fixed about 80 wrong facts, and five agents reworded about 300 sentences that read as generated.
> CI now runs a quick tier on every push, which leaves out 37 slow tests and the jobs a change does not need, and runs everything nightly and on a dispatch.

## State before

`37-node-editor` at `5b83271a` held the 99 signed commits of record #109's fold, pushed by force, the last of them remapping the hashes the records cite.
The maintainer had looked at the Editor in person and asked for the help marks as plain labels.
CI had run on the branch only by hand, and never with the graph and sheet work on Windows.

## What was done

### The requests

The maintainer's requests, in order:

1. "the question marks on editor pages should only be labels (not clickable, since they really aren't)", with a scripted offscreen recorder, the dark theme only, and the banner left as it is.
2. "We need a real logo", from the logo in the Figma file of the banner, "suitable for favicon, app icon etc.", and "properly designed aesthetically".
   The maintainer edited the logo in Figma and chose the Graphite variant.
3. A new demo video and gif. The first gif was "a bit large".
4. "Fix" the picker opened from a Cells-table chip that does not focus its number box, the three focus stops with no accessible name, World Width snapping 10000 to 10001, and the 5M ants that deliver no food, "and then make sure ci passes. Also check whether we can get PR #52 into our branch directly".
5. "check for docs consistency, before we merge this into main", then "also do a final pass on writing style against ai-like languages (similar to the string change we've done)".
6. "we should probably cut down long-running tests as well", with a faster check suite of about ten minutes on each push and the full suite before a release or at a fixed interval.

### Help marks, the icon and the demo

The "?" after a section title is a label with its help as a tooltip, and no longer a button (`25d10ab0`).

The logo came out of the Figma file as SVG, since a PNG export flattened it onto the section's ground.
`f451ce60` holds `assets/icon-1024.png`, the 256-pixel icons of the app and the template, the docs logo, the favicons at 16 to 64 pixels, the maskable icon at 512 and the iOS touch icon at 192.

A scripted recorder drove the app offscreen in the dark theme.
The scale chapters, one million boids and five million ants, were recorded on an RTX 5090 rented through Vast.ai.
The gif came down from 5.3 MB to 1.55 MB at 960 by 558 pixels and 12 frames a second, and the README links the maintainer's uploads of both (`6ad2e46e`).

### The four fixes and PR #52

- **Picker focus** (`a62df475`): every picker, the chance picker of a Cells-table chip included, opens with its number box focused.
  A long list keeps the box in a footer below it, and ↓ and ↑ from an empty box move to the items.
- **Tab stops** (`8caed5c9`): the three stops were two panel dividers and the empty state of no focus.
  Tab skips the dividers and wraps from the last control to the first, and Run to tick and History length got accessible names.
- **World Width** (`f5f48338`): egui's slider clamped a typed value, then snapped it from the minimum without clamping again.
  `params::snapped` keeps a typed bound as typed, and the slider rejects "NaN" and "inf", as `--set` does.
- **Ant Foraging in large worlds** (`1ac552e1`): a trail reaches `ln(reward / 1e-14) / -ln(update_cutdown)` cells, 306 at the defaults, so no ant finds food past a side of about 300 at any ant count.
  The CPU model, the GPU port and the starter agree with the reference model, so only the docs changed: the limit, the measurements, and settings that forage up to a side of 4,000.
- **PR #52** bumps pillow in `scripts/uv.lock` alone and applies cleanly, so it was cherry-picked as `8c4c3557`.
  GitHub marks the pull request as merged once the branch reaches master.

### CI on Windows

A dispatched run failed in two jobs.
The docs job failed on three intra-doc links that wasm32 builds cannot resolve, fixed by importing their targets under `cfg(any(doc, ...))` (`9d5a1a26`).
The Windows job failed in five crates, and a temporary branch `ci-debug-windows` ran the tests one at a time to find out why. It was deleted afterwards.

- **Line ends** (`2c5d5386`): a Windows checkout wrote CRLF into the WGSL and TOML that `include_str!` embeds, and about 160 tests compared that text with LF text.
  `.gitattributes` now checks every text file out with LF, and so does the template's.
  Four files committed with CRLF keep their bytes, `docs/license.html` among them.
- **The probe tallies** (`4ca3e07b`): with the CRLF fault gone, test processes still died with `STATUS_ACCESS_VIOLATION`.
  The debugger trace on the runner showed WARP's shader JIT writing through a bad pointer in `ComputeShaderTransformer::ConstructLoadStoreSets`.
  The trigger is an atomic on a storage buffer after a `workgroupBarrier()` inside the same `if`, even with a uniform condition, where the fire-count flush sat.
  It happens on the FXC path, which wgpu takes when no `dxcompiler.dll` is on the path, as on the runner.
  The barrier now sits at the top level of `main`, and the explicit zeroing of the tally went, since WGSL zeroes workgroup memory.
  Every model's statistics stayed identical.
- **abs** (`82279934`): FXC returned `-0.0` for `abs(-0.0)`. A `math.abs` of an `f32` or a `vec2<f32>` now clears the sign bit.

Run 45270855758 then passed every job, the first passing run since #37 began.
The Windows job took 93 minutes, since the whole suite now ran instead of crashing early.

### Docs consistency

Four reviewers each checked one set of files against the code at `82279934`: the App and Editor guides, the other guides with the authoring pages, the reference pages with the READMEs and the changelog, and the developing pages with `AGENTS.md` and `agents/`.
They fixed about 80 wrong facts.
Among them a sweep example that failed with "unknown parameter", eight crates counted instead of nine, GPU models said to reject every live edit, the Sheet pane's minimum width given as 320 points where the code has 160, and tried values in the preview, which no page described.
A fifth agent fixed the code comments that cited design notes, "the plan" or "Round 1", and renamed the tests named after milestones. No behaviour changed.

### Wording

Five agents reworded about 300 sentences of the published docs against the house rules and the catalogue of AI writing tells that the strings rewording of record #108 used.
They removed copula avoidance ("serves as"), trailing "-ing" and "which" clauses, "matters because", rhetorical questions, needless "Note that", and inflation such as "beautifully".
The tutorials keep their "we" and "let's".
A sixth agent compared every changed sentence with its old text and found three whose meaning had changed, all three fixed back to the facts.
A sweep put "rejects" for "refuses", "select" and "checked" for checkboxes, and "id" in lowercase across the pages, following the strings rewording.

The 101 commits of the review and the wording were folded into eight signed commits up to `d3a8c75b`, with the tree unchanged.

### A flaky kit test

Run 45301037979 failed `a_shared_accumulator_fails_the_thread_count_check` on Windows.
The test model's result depends on the order in which seven workers take a lock, and its work per agent is so small that one thread can run all 14 chunks in order before another worker wakes.
On the Windows runner, whose CPU also runs WARP, that happened on all five attempts.
The model now sleeps one millisecond at the start of each chunk.
Under full load on every core, the first attempt caught the race in 30 of 30 runs with the pause and 21 of 30 without it.

### Test tiers

The Windows job had grown to 93 minutes, and the maintainer asked for a check of about ten minutes on each push.
Per-test times from a nextest run on the Windows runner showed 21 tests of 60 seconds or more holding 67% of the test time.

- **The quick tier** runs on every push to any branch and on a pull request from a fork.
  It runs every test target but the benchmarks, with a `--skip` for each of the 37 tests in `scripts/slow_tests.txt`, each 20 seconds or more on WARP.
  The first quick run took 13.2 minutes on Windows with a threshold of 30 seconds, and the threshold came down to 20.
  The tests that caught this session's Windows faults stay in it.
  The doc tests and the facade's README program run on Linux alone.
- **The scope job** diffs a push against its previous commit.
  A change that only the docs site reads runs no workspace job, a change under `benchmarks/` runs `ports`, and a manifest change runs `package`.
  `msrv` and `features` now run on each push, and `docs` and `downstream` in the full tier alone.
- **The full tier** runs nightly on `master`, skipping a night with no new commit, and on a dispatch, which the release checklist already makes.
- **The fail-safe**: the quick tier skips under `--exact`, and `scripts/ci_tiers.py check` fails when a line of the list names no test or a test in more than one crate, so a renamed test cannot leave both tiers.
  Its first run caught a line that, by substring, also skipped henad-graph's `every_agent_sheet_lowers_checks_and_compiles_on_the_host`.
  The quick tier's tests ran in 55 seconds on an M4 Pro after its build.
- `./check.sh` still runs every test, and `./check.sh --quick` runs the quick tier's.

### Edited tree

```
.gitattributes                                 every text file checked out with LF, four CRLF files kept
.github/workflows/ci.yml                       the scope job, two tiers, the nightly schedule
AGENTS.md                                      the tiers, the quick check, the test folders, the ports job
CHANGELOG.md                                   the fixes, the icon, the facts the review corrected
README.md                                      the demo gif and video, Trunk
agents/                                        every topic file checked against the code, ci-and-release.md's tiers
assets/                                        the app icon at 1024, favicon, maskable and touch icons
check.sh                                       --quick
crates/
├── henad-app/
│   ├── assets/icon-256.png                    the new icon
│   └── src/
│       ├── ui/focus.rs                        new: Tab past dividers, the wrap from last to first
│       ├── ui/editor/sheet/picker.rs, ...     the number box focused, the footer, arrows from an empty box
│       └── ui/params/                         params::snapped, non-finite text rejected, names
├── henad-compute/src/{cpu,gpu}/sim_thread.rs  "Model rejected action" in the log
├── henad-explore/src/
│   ├── sweep/mod.rs                           doc links resolved on wasm32
│   └── tests/broken.rs, kit.rs                the shared accumulator's pause
├── henad-graph/src/
│   ├── wgsl/emit.rs                           the probe barrier at the top level, abs as a cleared sign bit
│   └── tests/catalogue.rs, ...                tests named for what they hold
└── henad-models/src/ants.rs, tests/           the trail's reach, the barrier test at the top level
docs/
├── assets/                                    the logo and favicon
├── developing/agent-record/20261124-110-…     this record
├── developing/contributing.md, releasing.md   the tiers, the full tier in the checklist
├── guide/, authoring/, reference/             the review's facts and the wording
└── reference/models.md                        Ant Foraging in larger worlds
scripts/
├── ci_tiers.py                                new: the tiers' cargo commands, the list check, a change's scope
├── slow_tests.txt                             new: the 37 tests the quick tier skips
├── test_ci_tiers.py                           new: tests of ci_tiers.py
└── uv.lock                                    pillow 13.0.0 (PR #52)
templates/model-project/                       LF line ends, the new icon, the README's wording
zensical.toml                                  this record's entry
```

## State after

`37-node-editor` holds the 21 signed commits up to `d3a8c75b` on the remote, then the shared accumulator's pause, the test tiers and this record, unsigned until the maintainer signs them.
Run 45270855758 passed every job at `82279934`, and the full local check passed 16 of 16 steps with 2,465 tests at `0e69b2c0`, a commit of the folded history.
The quick tier and the nightly run start once the tiers reach the remote, and the nightly run once they reach `master`.

## Issues found & future directions

### Left, with the options for each

- **"Fire" for actions.** The CLI, library and sweep pages keep "fire", since "run" would read "every run runs it".
  The App guide uses the app's own word, "run".
- **henad-graph in the provenance record.** A results folder records the versions and source hashes of henad-compute and henad-explore, not henad-graph's, so an edit to the WGSL compiler alone goes unrecorded.
- **The release token.** henad-graph is new since 0.3.0, so the next release creates it on crates.io and needs a token with the `publish-new` scope.
- **Design-note numbers.** About 44 "V-rule" numbers in henad-graph's checker cite a note outside the repository, and the `round1/` fixture folder is hash-pinned.
- **Smaller items.** The pages documenting 0.4 open with "Henad 0.3" notes until the release bumps them.
  Three limits stay out of `library.lock`, the Diagram uses colours outside the palette table, and `henad-app.md` and `henad-app-card.md` sit close to the 40,000-character limit of a topic file.
- **Selecting tests by crate.** The scope job selects jobs by path, and every workspace change runs the whole quick tier.
  Selecting crates by the dependency graph would save little: the slow tests are gone from the quick tier, and compiling dominates.
- **Faster slow tests.** The parity, conformance and band tests moved whole into the full tier.
  Shrinking their worlds or tick counts would touch calibrated bands, which is fixture work.

### Not checked

- The nightly schedule runs from `master` alone, so it starts after the merge.
- The scope job's diff of a force-push falls back to every quick job when the old head cannot be fetched. No force-push ran it yet.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
