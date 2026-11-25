---
date: 2026-10-12
title: "AGENTS.md split into topic files"
description: "AGENTS.md had grown to 210,708 characters, five times the 40,000 that Claude Code loads without a warning. Its core rules stay in a 33,057-character AGENTS.md, and the per-crate and per-subsystem detail moved verbatim into 19 files under agents/, with an index table pointing to them."
icon: material/file-tree-outline
status: ai-generated
model: claude-opus-5-5 (Claude Code)
state: "AGENTS.md at 33,057 characters, 19 topic files under agents/, every original line accounted for, uncommitted on 49-dynamic-loading"
baseline_commit: 8fae05f
delta_state: "uncommitted on `49-dynamic-loading`: AGENTS.md rewritten, agents/ added, this record and its nav entry"
---

# AGENTS.md split into topic files

> Claude Code reported that `AGENTS.md`, which the local `CLAUDE.md` imports, was over its size limit.
> The file held 210,708 characters, and its Architecture section alone held 138,830.
> `AGENTS.md` now keeps what every session needs (the writing rules, the working agreements, the commands, the lints and the crate graph) in 33,057 characters.
> The detail of each crate and subsystem moved verbatim into 19 files under a new `agents/` directory, which an index table in `AGENTS.md` lists.
> A script compared the old file with the new set line by line, and every line is in `AGENTS.md` or a topic file, apart from the headings the split replaced and two reworded lines.

## State before

`49-dynamic-loading` stood at `8fae05f` with a clean tree.
`AGENTS.md` ran to 2,489 lines.
The Architecture section held one bullet per crate, from 540 characters for henad-dylib to 41,509 for henad-app, followed by four subsections on models, performance, GPU traps and the sim runner.
Claude Code warns above 40,000 characters for a memory file, and loads the whole file into every session.

## What was done

### The split

The sections that matter to one area only moved out, each to a file of its own:

- each crate bullet of the Architecture section, its leading `- ` and two-space indent removed, so the text reads as a paragraph under the file's title
- henad-app's bullet in three files, since it alone passed 40,000 characters: the core of the app, the Editor tab, and the Sweep and Results tabs
- the full dependency rule, the four Architecture subsections, the UI writing rules, the documentation site's styling and include rules, and the CI, release, licence page and toolchain paragraphs of Commands
- the paragraph on the two places `unsafe` code sits, into `agents/dev-host.md` beside the loader it describes

Nothing in the moved text was condensed.
Six references that pointed "above" or "below" within the old file now name `AGENTS.md` or the topic file they mean.

### What stays in AGENTS.md

`AGENTS.md` keeps the sections every session reads in full: what Henad is, the session records, the cross-engine benchmark rules, the writing style apart from UI, the working agreements, the command block and quick commands, the environment variables, the lints and the crate graph.
New short text replaces what moved:

- a **Topic files** section that says how the files are used, and a table mapping each area to its file
- a summary of the documentation site, the region include kept as the example
- a summary of the dependency rule, and a list of the invariants that span crates (Struct-of-Arrays, no sequential twins, the chunk seed, the spatial hash and the scatter grid, macro drivers, the sim off the UI thread, the GPU submission bound)
- one paragraph on adding a model, pointing to `agents/models.md`
- one-line pointers for the UI rules, the CI jobs, releases, the licence page and the toolchain

The Markdown rule now names `agents/` beside `AGENTS.md` as hard-wrapped.

### Codebase structure

```
AGENTS.md                         # rewritten, 210,708 → 33,057 characters
agents/                           # new, one file per area, read on demand
├── ci-and-release.md
├── dependencies.md
├── dev-host.md                   # henad-hot, henad-dylib, hot-host, unsafe code
├── docs-site.md
├── facade-and-tutorial.md
├── gpu-traps.md
├── henad-app.md
├── henad-app-editor.md
├── henad-app-sweep.md
├── henad-build.md
├── henad-cli.md
├── henad-compute.md
├── henad-core.md
├── henad-explore.md
├── models.md                     # henad-models and adding a model
├── performance.md
├── sim-runner.md
├── template.md
└── ui.md
docs/developing/agent-record/
└── 20261012-59-agents-split.md   # this record
zensical.toml                     # nav entry for this record
```

## State after

`AGENTS.md` holds 33,057 characters, and the largest topic file, `agents/henad-explore.md`, holds 26,867.
The 19 topic files and `AGENTS.md` together hold about 216,000 characters, a little more than before, since the index and the summaries are new.
No line of the topic files passes 120 columns.
No code changed, and no check was run.
The change is uncommitted.

## Issues found & future directions

- **No tool loads the topic files on its own.** An agent reads one only when `AGENTS.md` points it there, so a session that skips the index misses the detail, such as the tests marked "Keep them".
  Nested `AGENTS.md` files in each crate directory would load automatically in some tools, but a crate's tarball would ship them unless its manifest excludes them, and `CLAUDE.md` is gitignored, so Claude Code would need local stubs.
- **The moved text was not trimmed.** Much of it lists test names and edge cases that the code and its tests already hold.
  Condensing it is a separate job, best done one area at a time with the maintainer.
- **`AGENTS.md` has about 7,000 characters of headroom.** New detail belongs in a topic file, and the index row for it is the only addition `AGENTS.md` needs.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
