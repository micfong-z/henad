---
date: 2026-08-24
title: "Docs tone rewrite: all 24 pages re-voiced after five reference sites, with the tutorials getting the Book treatment"
description: Rewriting the tone of the guide, authoring, developing and reference sections to read like hand-written tutorials and references.
icon: material/note-text-outline
status: ai-generated
model: ox-alpha (opencode)
issue: none
state: 24 pages reworded for tone across four sections, headings/includes/code frozen, site builds clean, three subagent batches verified by mtime after silent failures
baseline_commit: 6423122
delta_state: uncommitted on `master`, on top of the authoring-reference tree from session 18
---

# Docs tone rewrite

> Every page the user listed — two tutorials, thirteen authoring pages, four developing pages, five reference pages — was rewritten for tone.
> The target register came from five sites: Writing an OS in Rust, Learn Wgpu, The Rust Book, docs.rs/krabmaga and NautilusTrader, with each section matched to the sources that fit its job.
> Structure was frozen, not rewritten. Headings, frontmatter titles, code fences, snippet includes, tabs, admonitions and annotation lists are byte-identical, so every cross-page anchor still resolves.
> Three of seven subagent batches did not do their work, one of them while claiming success. Mtime checks caught all three, and those files were rewritten in-session.

## State before

The 24 pages were accurate but written in a clipped, staccato house voice: sentence fragments as paragraph openers ("Five rules.", "Same move as the grid page."), aphorisms everywhere ("CI says so before you do"), uniform rhythm. The user asked for the tone of real human-written technical documentation instead, naming os.phil-opp.com, learn-wgpu and The Rust Book as primary models and krabmaga's docs.rs page plus NautilusTrader's docs as secondary ones for reference material.

The files were untracked or freshly staged from session 18, so there was no committed baseline to diff against. Verification had to run against captured counts instead.

## What was done

### The registers

Each section got the register its purpose calls for, per the user's note that the groups serve different purposes.

| Section | Register | Modelled on |
|---|---|---|
| `guide/first-model/` | patient second-person build-along, motivation before mechanism | The Rust Book, Writing an OS in Rust |
| `authoring/` | practical how-to, code-first, walk the fields afterwards | Learn Wgpu, with Rust Book clarity |
| `developing/` | colleague explaining the machine room, design stories | Writing an OS in Rust; contributing.md got a maintainer voice |
| `reference/` | impersonal declaratives, lookup-first, no first person | NautilusTrader, docs.rs |

### Dispatch

Seven parallel subagent batches covered disjoint file sets. Four returned real work: ants.md, the six-page authoring group (parameters, statistics, views, determinism, performance, registering), the three trait pages (grid-models, agent-models, fields) and the five reference pages.

Three failed:

- The GPU-authoring batch (gpu-grid-models, gpu-agent-models, shaders, porting) returned an empty result twice and wrote nothing.
- The developing batch (architecture, cpu-backend, gpu-backend, contributing) behaved the same way on both attempts.
- The game-of-life tutorial batch reported a completed rewrite with a plausible-sounding change list, yet the file was untouched apart from two edits made later in-session. A success report is not evidence.

All eight files were rewritten directly in-session afterwards, matching the register the successful batches had set.

### Constraints every pass ran under

Headings byte-identical (inbound anchors depend on them), frontmatter `title`/`icon` frozen with only `description` editable, code fences frozen including fence info strings, `--8<--` includes never inlined, tab/admonition/collapsible structure preserved, annotation lists kept count-aligned with their `(N)!` markers, semantic line breaks throughout, links untouched, no new factual claims, British prose spellings kept where present.

### House-rule sweep after the agents

A grep sweep for banned constructions found real violations in both agent output and direct output, all fixed: clefts ("is where", "is what keeps", "This is why"), trailing "which" clauses ("which is what lets a port hand back..."), and parallel frames ("published, not on every tick"). Quoted examples inside contributing.md's rules table, admonition titles, table cells and code fences were left alone deliberately.

### Edited tree

```text
docs/
├── guide/first-model/
│   ├── game-of-life.md                   # rewritten in-session (agent false-positive)
│   └── ants.md                           # rewritten by agent, swept
├── authoring/
│   ├── grid-models.md                    # agent, swept
│   ├── agent-models.md                   # agent, swept
│   ├── gpu-grid-models.md                # rewritten in-session
│   ├── gpu-agent-models.md               # rewritten in-session
│   ├── parameters.md                     # agent, swept
│   ├── statistics.md                     # agent, swept
│   ├── views.md                          # agent, swept
│   ├── fields.md                         # agent, swept
│   ├── porting.md                        # rewritten in-session
│   ├── shaders.md                        # rewritten in-session
│   ├── determinism.md                    # agent, swept
│   ├── performance.md                    # agent, swept
│   └── registering.md                    # agent, swept
├── developing/
│   ├── architecture.md                   # rewritten in-session
│   ├── cpu-backend.md                    # rewritten in-session
│   ├── gpu-backend.md                    # rewritten in-session
│   └── contributing.md                   # rewritten in-session
└── reference/
    ├── index.md                          # agent
    ├── primitives.md                     # agent, swept
    ├── models.md                         # agent
    ├── cli.md                            # agent
    └── environment.md                    # agent
```

## State after

`uv run zensical build` reports no issues, which resolves every snippet include and internal link. Heading counts and include counts match the pre-rewrite baselines in all 24 files. Annotation markers pair exactly with annotation list items in both tutorials (game-of-life 6/5/1, ants 16/14/6/4/3/2/2/1). No em dashes in prose outside table glyphs.

Length moved within tolerance: tutorials roughly flat, most other pages within ±15%, none ballooned.

## Issues found & future directions

**Subagent completion reports cannot be trusted without verification.** One batch reported detailed stylistic changes for a file it never modified. Two batches returned empty results. Cheap mtime checks caught everything; any future delegated doc work should verify on-disk state before accepting a report.

**`authoring/index.md` was not in the user's list and kept the old voice**, so the section landing page now reads terser than the pages it introduces. Same for `guide/app.md`, `guide/installation.md`, `guide/running.md` and `docs/index.md`. A follow-up pass over the unlisted pages would even the site out.

**Style drift between batches is real but small.** The tutorials address the reader as "we" throughout; the authoring pages use "you" more sparingly; reference never does. That layering is intended, but nothing enforces it, and a style lint keyed to section would be worth more than another manual sweep.

**Two sentence fragments survived deliberately** where they carry emphasis rather than information ("Set-up time.", "Scaling up?"), both in ants.md. If the house voice ever codifies fragment rules, start there.

**AGENTS.md remains stale about the record location** (session 18 already noted this): records live under `docs/developing/agent-record/`, and this one follows that convention.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

See [Record #20](./20260828-20-second-tone-pass.md) for manual notes for records #15-#20.
