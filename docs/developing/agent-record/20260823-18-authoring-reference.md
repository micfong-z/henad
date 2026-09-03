---
date: 2026-08-23
title: "The authoring reference: every stub written, and the section regrouped around what a model actually declares"
description: Writing every work-in-progress authoring page, and regrouping the section.
icon: material/note-text-outline
status: ai-generated
model: claude-opus-5 (Claude Code)
issue: none
state: eleven work-in-progress pages written, three new pages added, authoring nav regrouped into four subsections, frontmatter completed across all 49 pages, site builds clean
baseline_commit: 6423122
delta_state: uncommitted on `master`, on top of the app-tour tree from session 17
---

# The authoring reference

> Eleven pages carried an `!!! failure "Work in progress"` block. All eleven are now written, except `benchmarks.md`, which the user chose to leave as a stub.
> The Authoring section was flat: ten pages in one list, with the four model traits interleaved with parameters, determinism and registration. It is now four named subsections, and the four `*-models` pages sit together under **Model traits**.
> Three pages are new. `views.md` and `shaders.md` fill real gaps, and `porting.md` answers a question the section could not previously answer at all.
> `parameters-and-stats.md` was split in two, since each half is a page's worth on its own.
> A second pass gave every page `title`, `description` and `icon`, which let 18 nav labels come out of `zensical.toml`.

## State before

The Authoring section was a flat nav list of ten entries, with the four trait pages scattered through it. Six of those ten pages were stubs, four of them entirely so.

| Page | State |
|---|---|
| `authoring/index.md` | Written |
| `authoring/grid-models.md` | Written, one WIP block at the end |
| `authoring/agent-models.md` | Written, one WIP block at the end |
| `authoring/gpu-grid-models.md` | Written, one WIP block at the end |
| `authoring/gpu-agent-models.md` | Written, one WIP block at the end |
| `authoring/fields.md` | WIP only, 11 lines |
| `authoring/parameters-and-stats.md` | WIP only, 13 lines |
| `authoring/performance.md` | WIP only, 16 lines |
| `authoring/determinism.md` | Half written, one WIP block |
| `authoring/registering.md` | Written |

`developing/cpu-backend.md` and `developing/gpu-backend.md` were WIP only, 12 and 13 lines. `developing/contributing.md` had the check commands and the AI-usage note, with a WIP block covering the house style, the registry tests and pull request bodies.

Nothing documented palettes, views, `prepare_view`, the generated shader bindings, or how to take a working CPU model to the GPU. The two tutorials in `guide/first-model/` covered a grid model and an agent model end to end, so the Authoring section was meant to be reference rather than a third tutorial.

Two links pointed at paths that no longer exist: `developing/index.md` and `developing/architecture.md` both linked the agent records to a GitHub tree, and the records have lived under `docs/developing/agent-record/` since session 15 moved them.

## What was done

### The nav

Five groups replace the flat list. Filenames stayed under `authoring/` rather than moving into subdirectories, so only the split page changed its URL.

```text
Authoring
├── Choosing a trait              authoring/index.md
├── Model traits
│   ├── Grid models               authoring/grid-models.md
│   ├── Agent models              authoring/agent-models.md
│   ├── GPU grid models           authoring/gpu-grid-models.md
│   └── GPU agent models          authoring/gpu-agent-models.md
├── What a model declares
│   ├── Parameters                authoring/parameters.md         # split out
│   ├── Statistics                authoring/statistics.md         # split out
│   ├── Palettes and views        authoring/views.md              # new
│   └── Fields                    authoring/fields.md
├── Going to the GPU
│   ├── Porting a model           authoring/porting.md            # new
│   └── Shaders and bindings      authoring/shaders.md            # new
└── Getting it right
    ├── Determinism and testing   authoring/determinism.md
    ├── Writing fast models       authoring/performance.md
    └── Registering a model       authoring/registering.md
```

`authoring/parameters-and-stats.md` is gone, replaced by the two pages above.

### The three new pages

**`views.md`** was the largest gap. Every one of the four traits declares a `PALETTE`, and nothing said what it was for. The page covers the palette, the two layers a model can publish, the `color = <lane>` line in `agent_lanes!`, `prepare_view`, the display cap and how a GPU model ends up carrying its colours twice.

**`shaders.md`** exists because both GPU trait pages needed the same material. `wgsl_bindgen`, the `#import` modules under `gpu/shared/`, the `linear_index` fold, `HENAD_DUMP_WGSL`, and what stays hand-maintained. Without it, both pages would carry a copy.

**`porting.md`** answers "I have a working CPU model, now what". It is the one task-shaped page in a section of reference pages, and it collects material that was scattered: reusing the counterpart's parameter list, confining the CPU `init` call to `seed_buffers`, what stays bit-identical per model and why the other three diverge, and the two device limits worth knowing before allocating.

### Snippet regions

Five new regions were marked in the sources, so five examples cannot drift from the code they document.

| Source | Region | Used by |
|---|---|---|
| `sir.rs` | `from_params` | `grid-models.md`, `parameters.md` |
| `boids/mod.rs` | `params` | `parameters.md` |
| `boids/lanes.rs` | `lanes` | `agent-models.md` |
| `gpu_boids/mod.rs` | `buffers` | `gpu-agent-models.md` |
| `gpu_boids/mod.rs` | `passes` | `gpu-agent-models.md` |

That is the whole of the Rust change: ten comment lines. Everything else was written out, following [session 16's finding](20260822-16-first-model-guide.md) that including every block forces a page to follow the file's order and rules out code annotations.

### One stale claim corrected

`gpu-agent-models.md` said a pass declares "a `&[Binding]` per pass whose slice index is the `@binding` index". That was true before session 8. Bindings are now generated from the shader at build time and resolved **by name**, against seven reserved names plus the model's own buffer labels. The page says so, and `shaders.md` carries the warning that name resolution cannot typecheck what the buffer actually holds.

### The two backend pages

`cpu-backend.md` covers the two engines, the fixed order of an agent tick, the peeled x-wrap and why the interior loop uses `enumerate()`, why `for_each_chunk_mut!` has to stay a macro, the seeding pair, and the `SimLoop`/`Driver` split. That split is newer than `AGENTS.md`'s description of the wasm path, which still says `SimThread::update()` is called from `eframe::App::update()`.

`gpu-backend.md` covers the engines, the four primitives, batching, the two principles behind `limits.rs::raise`, the three layers of failure handling, and the four traps that already cost debugging.

### Reflow

Every page written this session was reflowed to one line per sentence after the fact. The first drafts were hard-wrapped at roughly 100 columns, which is what `AGENTS.md` forbids and what `guide/first-model/ants.md` does not do. A script joined continuation lines and re-split on sentence boundaries, and the result was checked byte-for-byte against the originals modulo whitespace.

### Edited tree

```text
crates/
├── henad-models/src/
│   ├── sir.rs                          # snippet region: from_params
│   ├── boids/lanes.rs                  # snippet region: lanes
│   ├── boids/mod.rs                    # snippet region: params
│   └── gpu_boids/mod.rs                # snippet regions: buffers, passes
docs/
├── authoring/
│   ├── index.md                        # links updated, tutorial cards added
│   ├── grid-models.md                  # WIP block replaced
│   ├── agent-models.md                 # WIP block replaced
│   ├── gpu-grid-models.md              # WIP block replaced
│   ├── gpu-agent-models.md             # WIP block replaced, binding claim corrected
│   ├── fields.md                       # written
│   ├── parameters.md                   # new, split from parameters-and-stats.md
│   ├── statistics.md                   # new, split from parameters-and-stats.md
│   ├── views.md                        # new
│   ├── porting.md                      # new
│   ├── shaders.md                      # new
│   ├── determinism.md                  # WIP block replaced
│   ├── performance.md                  # written
│   ├── registering.md                  # table and links added
│   └── parameters-and-stats.md         # deleted
├── developing/
│   ├── architecture.md                 # agent-record link fixed, backend links added
│   ├── cpu-backend.md                  # written
│   ├── gpu-backend.md                  # written
│   └── contributing.md                 # WIP block replaced
├── benchmarks.md                       # left as a stub, on the user's call
├── reference/*.md                      # frontmatter added, five files had none at all
└── developing/agent-record/2026*.md    # icon and description added, 18 files
zensical.toml                           # authoring nav regrouped, 18 labels removed
```

### Frontmatter, and the nav labels it replaces

Every page now carries `title`, `description` and `icon`, in that order, 49 of 49. The five `reference/` pages had no frontmatter at all, and the 18 session records had `title` but neither of the other two.

A nav entry spelling out a label the page's own frontmatter already gives is a second place for that name to live. Eighteen were removed and the paths now stand bare.

```toml
{ "Grid models" = "authoring/grid-models.md" },   # before
"authoring/grid-models.md",                       # after
```

Two kinds of entry keep their label, both deliberately. `Roadmap` points at an external URL and has no page to read a title from. The record entries read `#01, 2026-08-12` rather than the long sentence in each record's `title`, which is the split `guide/first-model/ants.md` already uses: frontmatter `title` is the nav label and the browser tab, the `#` heading is the page's own.

Three pages were retitled so their nav entry could go. `authoring/porting.md` became "Porting a model", `reference/primitives.md` "Primitives" and `reference/models.md` "Default models". All three keep their longer `#` heading.

## State after

`uv run zensical build` reports no issues. `cargo fmt --all -- --check`, `cargo check --workspace --all-targets` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` all pass.

Every internal link and every anchor was resolved against the built site, fourteen cross-page anchors included. Every page was checked in a browser for the constructs it uses: tables, admonitions, content tabs, definition lists, grid cards and the five snippet includes. No page carries a leftover `--8<--` marker.

Claims were checked against the source rather than against `AGENTS.md`, which is stale in three places noted below.

The dev server was started twice and stopped both times, with `pgrep` and `lsof` confirming nothing was left listening on port 8000.

## Issues found & future directions

**`benchmarks.md` is the one page still stubbed.** The only measured data in the tree is `results/7_bench_matrix_4090_38d0436.csv`, gitignored, taken 11 commits back. Its numbers are real and internally consistent, and one of them needs framing before publication: GPU Game of Life reports a flat ~128k TPS from a 0x0 grid all the way to 4096², which means the CPU-side encode dominates below 8192² and the headline "2.15 trillion cell-updates/sec" at 4096² is not a GPU measurement. Only the 8192² point is work-bound. Nothing in the repository records what CPU that machine has, which a throughput table needs.

**`AGENTS.md` is stale in three places.** It says session records go under `agent-record/` at the repository root; they live under `docs/developing/agent-record/`. It describes the wasm sim path as `SimThread::update()` called from `eframe::App::update()`; that is now the `runner::frame::Driver` half of the `SimLoop`/`Driver` split. And its `henad-app/ui/` file list still names `toolbar.rs` and `sidebar.rs`, as [session 17](20260822-17-app-tour.md) already noted.

**Splitting `parameters-and-stats.md` breaks a published URL.** `/authoring/parameters-and-stats/` now 404s. Zensical has no redirect support configured, and nothing in the repo tracks external links into the site.

**The Authoring pages and the two tutorials overlap.** `fields.md` and `guide/first-model/ants.md` both explain the scatter, and `agent-models.md` and the same tutorial both explain `dual` against `plain`. The split is deliberate, reference against walkthrough, but the two will drift independently and nothing checks that they agree.

**Nothing checks a doc claim against the code.** The five snippet regions pin five examples. Everything else, the binding tables, the parameter index layouts, the limit numbers, the tick order, is prose that can go stale silently. The stale `&[Binding]` claim corrected this session sat wrong since session 8, through two sessions that touched the same page.

**`docs/index.md` linked a page that had been deleted.** The Developing section lost its `index.md` partway through the session, and the quick-links card on the home page still pointed at it. Zensical's build caught it, and the card now points at `developing/architecture.md`. Nothing else linked the deleted page.

**The reflow script is not committed.** It lives in the scratchpad. A markdown lint that fails on a hard-wrapped prose line would be a better answer than a one-off script, and would catch the next page written the wrong way.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

See [Record #20](./20260828-20-second-tone-pass.md) for manual notes for records #15-#20.
