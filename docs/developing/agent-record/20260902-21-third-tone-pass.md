---
date: 2026-09-02
title: "Third tone pass: 23 pages matched to the CPU grid tutorial's voice"
description: A rewrite of the authoring, reference and developing sections against five named prose tics, with structure machine-verified per file.
icon: material/note-text-outline
status: ai-generated
model: claude-opus-5 (Claude Code)
issue: none
state: 23 pages re-voiced with headings, code fences, includes and tables verified against a recorded baseline, ten "What…"-led headings renamed with their inbound links updated, five broken table rows repaired, site builds clean
baseline_commit: 4577756
delta_state: uncommitted on `master`, on top of sessions 19 and 20's uncommitted tree
---

# Third tone pass

> The user named one target voice, the CPU grid model tutorial, and five things to avoid: parallel sentence structure above all, AI-flavoured word choice, "What…"/"How…"-led phrasing, a repeating "X, so Y" cadence, and sentences that open on a heavy noun phrase instead of the thing being talked about.
> All fourteen authoring pages, the five reference pages and the four developing pages were rewritten against that brief.
> A structural verifier ran per file, comparing headings, fence bodies, include lines, tab blocks, annotation markers and table shape against a baseline recorded before any edit.
> Five table rows turned out to be broken by an earlier semantic-line-break pass, and repairing them was the only content change beyond voice.

## State before

Sessions 19 and 20 had rewritten these pages twice, and the working tree held that result uncommitted.
The prose was accurate, but three tics ran through it.
`rather than` appeared 71 times across 23 files and `, so` 106 times, both often twice in a neighbouring pair of sentences.
Eight headings and six frontmatter descriptions used the "what/why/whether" register `AGENTS.md` rules out for comments.
Two of the user's own examples came from this tree: "Two implementations ship with the engine, `gpu_game_of_life/` and `gpu_sir/`" and "The two implementations to read are `gpu_boids/` and `gpu_ants/`".

Session 20's pass had also broken five table rows by putting a semantic line break inside a table cell, which ends the row.
Three sat in `authoring/grid-models.md`, two in the prose-style table in `developing/contributing.md`.

## What was done

### Scope settled first

The brief said "authoring, reference and developing (except for the About record)", which reads either as the four developing pages or as those plus the twenty session records.
The user confirmed the four pages, so the records were left alone.

### The rewrite

Every page was rewritten in full, apart from `reference/primitives.md`, `models.md`, `cli.md`, `environment.md` and `index.md`, which are mostly reference tables and took sentence-level edits instead.

| Tic | Before | After |
|---|---|---|
| `rather than` | 71 | 37 |
| `, so` | 106 | 53 |
| `which means` | 11 | 0 |
| `because` | 67 | 29 |

Ten headings lost the "What…"-led or antithetical shape.
`## What the engine does` became `## Left to the engine` in two files, `## What the trait asks for` became `## Items you supply`, `## What stays identical, and what does not` became `## Tick 0 and after`, and `## Reuse the palette, not the colours` became `## Reuse the palette`.
`## What the registry already checks` became `## Tests the registry brings`, whose anchor two other pages link to, so both links were updated in the same pass.
Six frontmatter descriptions opening on "How to …" were reworded to noun phrases.

Table column headers reading `What it is` or `What it does` became `Role`, `Contents`, `Pins` or `Effect`.

### Verification

A Python verifier compared each rewritten file against a baseline copied before any edit, requiring headings, code fence info strings and bodies, `--8<--` include lines, tab markers, annotation markers, admonition counts, table row and column counts and the frontmatter `title` and `icon` to stay identical.
Every reported difference was inspected: the ten heading renames and the five repaired table rows are the complete list.

A second check confirmed semantic line breaks held everywhere, with no line carrying two sentences outside code, tables and lists.

`uv run zensical build` reports no issues.
One round of broken links appeared partway through, when Zensical Studio rewrote three relative links after a file was moved between directories; those were put back by hand.

### Edited tree

```text
docs/
  authoring/
    index.md              rewritten, heading renamed
    grid-models.md        rewritten, two headings renamed, three table rows repaired
    agent-models.md       rewritten, heading renamed
    fields.md             rewritten
    parameters.md         rewritten
    statistics.md         rewritten
    views.md              rewritten
    determinism.md        rewritten, heading renamed
    performance.md        rewritten
    registering.md        rewritten, heading renamed
    gpu-grid-models.md    rewritten
    gpu-agent-models.md   rewritten
    shaders.md            rewritten, two headings renamed
    porting.md            rewritten, two headings renamed
  reference/
    index.md              sentence edits
    primitives.md         sentence edits
    models.md             sentence edits
    cli.md                sentence edits, one table header
    environment.md        one sentence
  developing/
    architecture.md       rewritten
    cpu-backend.md        rewritten
    gpu-backend.md        rewritten
    contributing.md       rewritten, two table rows repaired
    agent-record/
      20260902-21-third-tone-pass.md   this record
zensical.toml             nav entry for this record
```

## State after

The three sections read in the tutorial's voice: ordinary word order, connectives varied across a page, and no heading circling its subject with a headless clause.
Structure is unchanged except where the change was the point, and the site builds with no warnings.

The About record, the guide, the tutorials, `benchmarks.md` and the twenty session records were not touched.

## Issues found & future directions

- The five broken table rows had been live since session 20, which suggests a "one sentence per line" sweep needs a table-aware guard.
  The verifier written here catches the shape and could run in CI over `docs/`.
- Zensical Studio rewrites relative links when a file moves between directories.
  Writing a page straight to its final path avoids it.
- `reference/primitives.md` is largely tables, and its prose is now a small fraction of the page.
  A future pass should check the table cells themselves, which still carry a mixed register.
- Two `zensical serve` processes were already running when this session started and were left alone.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

