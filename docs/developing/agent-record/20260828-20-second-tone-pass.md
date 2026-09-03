---
date: 2026-08-28
title: "Second tone pass: 25 pages re-voiced from style dossiers, with every batch verified on disk"
description: A second rewrite of the guide, authoring, reference and developing sections toward the registers of five named documentation sites.
icon: material/note-text-outline
status: ai-generated
model: claude-fable-5 (Claude Code)
issue: none
state: 25 pages re-voiced across four sections with structure frozen and machine-verified per file, two new GPU tutorials added to the first-model guide with all code included from the shipped sources, site builds clean
baseline_commit: 4577756
delta_state: uncommitted on `master`, on top of session 19's uncommitted tree
---

# Second tone pass

> Every page the user listed was rewritten for tone a second time: the two tutorials, all fourteen authoring pages (this time including `authoring/index.md`), the five reference pages, and the four developing pages.
> Session 19 had already re-voiced most of these, but the result kept a clipped, aphoristic register, so this pass went further toward the named sites: connected sentences, explicit connectives, and no epigrams.
> Five style dossiers were gathered first, one per reference site, and distilled into written briefs that seven rewrite subagents worked from.
> Every batch was verified on disk against recorded md5 and structural baselines before being accepted, a direct response to session 19's silent subagent failures.

## State before

Session 19 had rewritten 24 of these pages once, and the working tree held that result uncommitted.
The pages were accurate and structurally sound, but the voice was still recognisably synthetic: staccato fragments as beats ("Set-up time.", "The cheap half."), aphoristic closers ("CI speaks up before you do"), clever inversions ("one fact written twice"), and rhetorical questions as section pivots ("Why fold?", "Why two functions?").
The user asked for the same target sites as before, os.phil-opp.com, Learn Wgpu and the Rust Book as primary with krABMaga's docs.rs page and NautilusTrader as secondary, and again noted that the doc groups serve different purposes and need slightly different styles.
`authoring/index.md` was newly in scope this time, having kept the old voice after session 19.

Record #19 existed on disk but had no nav entry in `zensical.toml`, so nothing linked to it.

## What was done

### Style dossiers before any rewriting

Five subagents each read four to six substantial pages of one reference site and returned a dossier: voice and person, sentence shape, transitions, code choreography, what the prose never does, and instructions for imitating the register without copying.
The dossiers converged on a useful diagnosis: all five sites build meaning through connection (because, so that, otherwise, however) where the existing pages built it through compression, and none of them ever write a sentence intended to be admired.

The dossiers were distilled into five briefing files the rewrite agents read before touching anything: a common-constraints brief, one register brief per section, and a digest of the dossiers.

| Section | Register | Modelled on |
|---|---|---|
| `guide/first-model/` | shared-work "we", complete sentences, compiler errors as teaching beats | Rust Book, phil-opp |
| `authoring/` | "you" to the model author, code first then unpacked, gotchas in admonitions | Learn Wgpu, Rust Book concept chapters |
| `reference/` | impersonal present-tense declaratives, verb-first fragments in tables | docs.rs, std docs |
| `developing/` | system in third person, consequence-shaped rationale, graded modals in contributing | NautilusTrader, phil-opp |

### Dispatch and verification

Seven rewrite batches ran in parallel: one per tutorial, three across authoring, one for reference, one for developing.
All seven returned real work this time.
Verification did not take their word for it: a script compared every file against baselines recorded before dispatch, requiring the md5 to change while heading text, fence bodies, `--8<--` include counts, annotation-marker counts, admonition counts and tab counts stayed identical.
All 25 files pass.

### The sweep after the agents

The batches shared one over-correction: the old ", so" tic came back as "which means", six times in the Game of Life tutorial alone.
A cross-file sweep thinned that density by hand, varied adjacent connectives, and confirmed no banned phrase survived outside quoted example content, no em dashes entered prose, one rhetorical question at most per page remained, and semantic line breaks held everywhere, including in the developing pages that previously had multi-sentence lines.

One factual fix rode along: `developing/cpu-backend.md` claimed the row loop uses `enumerate()` "because `enumerate()` measures worse", contradicting itself.
The sentence now says the `zip()` form measures worse, matching `AGENTS.md`.

### Two GPU tutorials, added on a second request in the same session

After the tone pass, the user asked for GPU counterparts to the two first-model tutorials, and by then had renamed the Life page to "CPU Grid model" by hand.
Two new pages went in under `guide/first-model/`, voiced in the same tutorial register:

- `gpu-game-of-life.md` walks the shipped `gpu_game_of_life`: the three-shader shape, the bit-packed state layout and the one-invocation-per-word ownership rule, the SWAR carry-save step shader explained adder by adder, the sampled display, the two-level reduce, and CPU-seeded tick 0 as the correctness oracle.
- `gpu-ants.md` walks the shipped `gpu_ants`: the pass list, the seven in-place buffers against the eight-binding budget, the packed state word and the reward-is-two-valued fact that permits it, the fused step kernel with its `atomicMax` deposit, the merge pass as the synchronisation point, and the replay-but-diverge determinism story with its invariant oracles.

Both pages take a different stance from the CPU tutorials: instead of a build-along backed by a CI-compared tutorial module, they are guided walks of the shipped source, with **every listing pulled in by snippet include** so the pages cannot drift.
That required adding `--8<--` region markers (comment-only) to `gpu_game_of_life/mod.rs` and `step.wgsl`, `gpu_ants/mod.rs` and `step.wgsl`, `registry.rs` (the GPU entries) and `build.rs` (the shader entry-point list).
Markers in WGSL are ordinary line comments and survive `wgsl_bindgen`; `cargo check -p henad-models` and `cargo fmt --check` pass with them in place.
The CPU ants page's Next section now links both new pages, and `zensical.toml` lists them under "Writing your first model".

### Structure held constant

Headings byte-identical (inbound anchors depend on them), frontmatter `title` and `icon` frozen with `description` editable, code fences byte-identical including info strings, includes never inlined, annotation lists count-aligned with their `(N)!` markers, tables row-and-column identical, links untouched, British prose spellings kept, no new factual claims beyond the one correction above.

### Edited tree

```text
docs/
├── guide/first-model/
│   ├── game-of-life.md                   # agent, then which-means sweep in-session
│   ├── ants.md                           # agent; Next section later links the GPU pages
│   ├── gpu-game-of-life.md               # NEW, walks the shipped gpu_game_of_life
│   └── gpu-ants.md                       # NEW, walks the shipped gpu_ants
├── authoring/
│   ├── index.md                          # agent (newly in scope this session)
│   ├── grid-models.md                    # agent, then sweep in-session
│   ├── agent-models.md                   # agent
│   ├── gpu-grid-models.md                # agent
│   ├── gpu-agent-models.md               # agent
│   ├── parameters.md                     # agent
│   ├── statistics.md                     # agent
│   ├── views.md                          # agent
│   ├── fields.md                         # agent
│   ├── porting.md                        # agent
│   ├── shaders.md                        # agent
│   ├── determinism.md                    # agent
│   ├── performance.md                    # agent, one clause smoothed in-session
│   └── registering.md                    # agent
├── reference/
│   ├── index.md                          # agent
│   ├── primitives.md                     # agent
│   ├── models.md                         # agent
│   ├── cli.md                            # agent
│   └── environment.md                    # agent
└── developing/
    ├── architecture.md                   # agent
    ├── cpu-backend.md                    # agent, includes the zip()/enumerate() fix
    ├── gpu-backend.md                    # agent
    └── contributing.md                   # agent
zensical.toml                             # nav: records #19 and #20, the two GPU tutorials
crates/henad-models/
├── build.rs                              # snippet region around ENTRY_POINTS
└── src/
    ├── registry.rs                       # snippet region around the GPU entries
    ├── gpu_game_of_life/{mod.rs,step.wgsl}   # snippet regions, comment-only
    └── gpu_ants/{mod.rs,step.wgsl}           # snippet regions, comment-only
```

## State after

`uv run zensical build` reports no issues, resolving every snippet include and internal link, the new GPU pages' region includes among them.
All 25 rewritten files pass the structural verification: content changed, structure identical to the pre-rewrite baselines.
The guide section now covers all four authoring traits end to end, two build-alongs and two guided walks.
The tutorials now read as shared-work build-alongs, the authoring pages as practical how-tos addressed to the author, the reference pages as impersonal contract prose, and the developing pages as third-person internals notes with a maintainer-voiced contributing guide.
Record #19 and this record are both linked from the nav.

## Issues found & future directions

**Rewrite agents trade one tic for another.**
Told to remove ", so" chains and staccato fragments, several batches leaned on "which means" instead, and the orchestrating sweep had to redistribute connectives afterwards.
A brief that names the replacement palette explicitly (because, otherwise, two sentences, folded phrase) would reduce that follow-up work.

**Verification against recorded baselines worked.**
Requiring the md5 to change while seven structural counts stay fixed caught nothing this time, but it is what would have caught session 19's false-success batch, and it costs one script.
Keep it for any future delegated doc work.

**The unlisted pages still carry the older voice.**
`docs/index.md`, `guide/installation.md`, `guide/running.md`, `guide/app.md` and `benchmarks.md` were out of scope in both tone passes and now sit one register away from the sections around them.

**Session records themselves are unregulated.**
The records are published but were never named in either rewrite request, and their voices now span three sessions of house styles.
Probably fine for hand-off documents, but worth a decision at some point.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

So a human chose Zensical and considered alternatives like mdBook, then listed a set of pages to write.

Many parts were rewritten by a human (as well as by agents when asked by a human).
Unfortunately, it is currently impractical to write every single word in this documentation by hand due to time constraints.
Nevertheless all documentation are read and checked by a human. This is taking 2+ weeks so far...

References were chosen by my personal preference ;)
