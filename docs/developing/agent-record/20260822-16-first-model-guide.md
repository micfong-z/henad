---
date: 2026-08-22
title: "Writing your first model: two ground-up tutorials, and a compiled twin that holds them to the engine"
description: Two ground-up model tutorials, with a compiled twin holding them to the engine.
icon: material/note-text-outline
status: ai-generated
model: claude-opus-5 (Claude Code)
issue: none
state: both pages written and verified in a browser, twin and parity tests green, `./check.sh` green
baseline_commit: 6423122
delta_state: uncommitted on `master`, on top of the docs-site tree from session 15
---

# The first-model guide

> The two pages the nav promised under "Writing your first model" now exist, one per CPU topology, and both are written ground-up from an empty file rather than assembled out of snippets.
> The reader ends each page holding a complete, correct model, and `crates/henad-models/src/tests/tutorial/` holds that same code as a real module, stepped beside the shipped model by a parity test that demands identical bits.
> The first draft went the other way, including every block from the shipped sources through `pymdownx.snippets`. It was abandoned once two costs became clear: the page has to follow the file's order, and `--8<--` blocks cannot carry code annotations.
> Zensical's feature set was probed rather than assumed, and mermaid was wrongly written off partway through on the strength of a browser harness that never finished mounting it.

## State before

`zensical.toml` listed `guide/first-model/game-of-life.md` and `guide/first-model/ants.md` in the nav, and neither file existed.

`docs/authoring/` covered the same two traits in reference form, in 25 and 52 lines, both ending in a "work in progress" admonition promising a walkthrough.

One snippet region existed in the whole workspace, `game_of_life.rs:step_cell`, pulled by `docs/index.md` and `docs/authoring/grid-models.md`.

## What was done

### Probing the builder

A scratch page was built and read back in a browser rather than trusting Material's documentation, since Zensical implements a subset.

| Feature | Result |
|---|---|
| Code annotations | Work, client-side, via a `.annotate` class on the block or the `content.code.annotate` feature |
| `hl_lines`, `linenums`, `title=` | Work |
| Snippets in a titled fence, several regions per fence | Work |
| `dedent_subsections` | Works, and is now on |
| Content tabs, admonitions, `???` details, definition lists, grid cards | Work |
| Inline `#!rust` highlighting | Works |
| Mermaid | Works |

`content.code.annotate` was tried as a global feature and dropped in favour of the per-block `.annotate` class.
Global annotations turn any code block followed by an ordered list into an annotation host, and both pages have prose lists directly under code.

Mermaid was briefly and wrongly written off.
The theme's auto-mount left an empty `<div class="mermaid">` in the agent's browser pane on every page after the first, while `mermaid.render()` called by hand from the same console returned valid SVG every time.
The maintainer corrected the call. Every diagram source on both pages was then validated through that hand call before being committed to a page.

### The first draft, and why it went

Both pages were written once with every Rust block included from the shipped sources, which took 30 marker pairs across `game_of_life.rs`, the four `ants/` files and `registry.rs`.
Two costs decided against it.

A snippet can only show a file's final state, so the page has to follow the file's order rather than a learner's.
The Game of Life page ended up burying `step_cell`, which is the entire model, two thirds of the way down behind the palette and the parameter list, because that is where the file puts it.

Annotation markers cannot live in a `.rs` file, so every real code block fell back to `hl_lines` plus a paragraph, and the reader had to map a line number to prose several inches below.
The only blocks on either page that read well were the two hand-written trait skeletons, which were the only two carrying annotations.

The 30 marker pairs were removed again. `game_of_life.rs`'s `step_cell` stays, since two other pages pull it, and one new pair marks the registry list.

### The twin

`crates/henad-models/src/tests/tutorial/` holds the finished state of each page as a real module, unregistered, compiled and tested under `cargo test`.

`parity.rs` steps each twin beside the model it teaches and compares raw bits.
Game of Life over 200 ticks on a deliberately non-square grid compares every cell and every stat value.
Ants over 400 ticks compares every position, the colour lane, the delivery tally, both pheromone grids, the quantised display layer and all three statistics, plus a separate check that `build_sites` lays out the same obstacles.
Both also compare declared parameter descriptors, and ants compares `CHUNK`, since it sets the rng seeding granularity.

Each comparison carries a second assertion that the run was not trivial, so a test cannot pass by comparing two empty grids.

The guard was confirmed by flipping `(ALIVE, 2..=3)` to `2..=4` in the twin, which failed with `docs/guide/first-model/game-of-life.md no longer produces the shipped model` and left the other four passing.

The twins use ids `life` and `foraging` rather than `game_of_life` and `ants`.
`henad-cli` resolves a model by the first id that matches, so a reader registering their version alongside the shipped one needs a distinct id, and both pages say so.

### The pages

Each page opens on an empty file and grows it in the order a person would actually write it.

Game of Life starts with the rule as a free function, before the trait exists, because the rule is the only thing the reader knows yet.
It then adds an empty `impl` and lets `cargo check` list the twelve missing items, with the real `E0046` output on the page.
Statistics and parameters are stubbed so the model can be registered and run halfway down the page, and only then filled in.

Ants starts with state, because the trait's associated types name the lane and field types.
It writes the second pass before the first, since the deposit rule only makes sense once you know what the movement pass is looking for.

Imports arrive as the code needs them rather than in a block at the top, which is what the maintainer asked for and is also the only honest way to present a file that grows.

A line-level check was run over both pages, extracting every fenced block whose title names a tutorial file and confirming each line appears in the corresponding twin.
It found four real drifts: a comment dropped from the twin's `quantize`, a `scalar` import shown twice instead of widened, a determinism test on the page that existed nowhere in the workspace, and two tests missing the `use` lines a reader would need.
All four are fixed, and the only lines now unaccounted for are the deliberate intermediate states.

```
henad/
├── zensical.toml                                      # dedent_subsections, record #16 in nav
├── docs/guide/first-model/
│   ├── game-of-life.md                                # new, ground-up
│   └── ants.md                                        # new, ground-up
└── crates/henad-models/src/
    ├── registry.rs                                    # one region: cpu_entries
    └── tests/
        ├── mod.rs                                     # + pub mod tutorial
        └── tutorial/                                  # new
            ├── mod.rs
            ├── life.rs                                # the grid page, finished
            ├── foraging/{mod.rs, field.rs}            # the agent page, finished
            └── parity.rs                              # 5 tests against the shipped models
```

## State after

Seven tests cover the tutorials: five parity, the blinker the grid page teaches, and the thread-count test the agent page teaches.

`./check.sh` passes, including the wasm typecheck and the web build.
The site builds with no issues and no broken links.

Both pages were opened in a browser and checked structurally. The grid page carries 12 annotations across 6 blocks, the agent page 48 across 15, and every annotation list is consumed rather than left rendering as prose.

The `ants/` sources are byte for byte back to where the session found them.

## Issues found & future directions

**The twin duplicates two shipped models.** That is the price of hand-written tutorial code, and it buys a page that cannot silently rot. It is paid in a test-only tree that never reaches the app or the registry. Worth revisiting only if a third tutorial makes it three copies.

**Page-to-twin transcription is checked by hand.** The parity tests catch the engine moving under the page. They do not catch a typo made while transcribing the twin into markdown. The throwaway line-level script that found four drifts could become a real test that parses both pages and diffs the fenced blocks against the twin.

**The authoring pages now duplicate the tutorials.** `grid-models.md` and `agent-models.md` were written as the walkthrough that did not exist yet. They should become the reference half, pointing at the tutorial for the narrative, and drop the promises their "work in progress" blocks make.

**Three stubs the tutorials lean on are still empty.** `authoring/fields.md`, `authoring/parameters-and-stats.md` and `authoring/performance.md` are all linked from the new pages and all say "work in progress". The agent page now carries most of the field and scatter material `fields.md` was going to hold.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

See [Record #20](./20260828-20-second-tone-pass.md) for manual notes for records #15-#20.
