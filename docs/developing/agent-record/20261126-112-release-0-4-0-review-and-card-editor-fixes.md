---
date: 2026-11-26
title: "Release 0.4.0: a review, dependencies and card editor fixes"
description: "After #37 merged into master, the maintainer asked for release 0.4.0 with every document, licence and related file reviewed first, a benchmark rerun on newer hardware, the latest egui and dependencies, and fixes for 11 card editor problems found by trying the app. A review of the release found 81 faults, one of them an unlisted breaking change. The dependency patches brought a slider regression, found and fixed before release. A read-only search found 15 more editor problems, and the maintainer chose 11 of them and a regrouped New model dialog for this release."
icon: material/tag
status: ai-generated
model: "Claude Code. Agents made the review, the fixes, the dependency upgrade, the editor changes and the screenshots, and the session wrote this record"
issue: "release 0.4.0"
state: "master holds the release's 57 commits after ae5d5fad, this record included, and the full local check passed on 113c0b9c. The tag, the full CI tier and the publish follow the release checklist"
baseline_commit: ae5d5fad
delta_state: "committed on `master` after `ae5d5fad`, signed before the push of `release-0.4.0`"
---

# Release 0.4.0: a review, dependencies and card editor fixes

> With #37 merged, the maintainer asked for release 0.4.0, and for every document, licence and related file to be reviewed first so that nothing is left behind.
> A review with adversarial checking found 81 faults across the docs, the changelog, the crate pages, the template and the release procedure, one of them a breaking change the changelog did not list.
> The maintainer then asked for three more things before the tag: a benchmark rerun on newer hardware, the latest egui and dependencies, and fixes for 11 card editor problems found by trying the app.
> The benchmark rerun is record [#111](20261125-111-rtx-4090-rerun-on-a-9950x3d.md).
> egui 0.37 cannot be taken yet, and the patch releases within 0.36 and wgpu 30 brought one regression, which typing into the Parameters tab found.
> A read-only search for problems like the maintainer's found 15 more, and the maintainer chose 11 of them, together with a New model dialog that keeps the grid rule and node graph starters under Advanced.

## State before

`master` was at `ae5d5fad`, the maintainer's README commit after the merge of #37 into `master`.
The version was 0.3.0 on crates.io, and henad-graph had never been published.
`docs/developing/releasing.md` described a release made on `master` directly, and the CI tiers of record [#110](20261124-110-fixes-windows-ci-docs-review-and-test-tiers.md) were new.

## What was done

### The review

Finders read every published page, the changelog, the crate READMEs and crate docs, the template, the licence files and the release procedure against the code, and each finding went to verifiers told to refute it.
81 findings survived, and a completeness critic added one more.

- **A breaking change the changelog did not list.** `SweepOptions` no longer implements `UnwindSafe`. The changelog now lists it under Breaking, with a migration note.
- **The release procedure.** `releasing.md` now bumps on a `release-0.x.y` branch and runs the full tier on it before `master` moves.
  It also gains stability lists for henad-graph, model files and the new `non_exhaustive` types, and the deprecation rule amended at the maintainer's choice.
- **The template.** Its own files are dedicated to the public domain under CC0 1.0, the Henad icon excluded, and `git init -b main` replaces a bare `git init`.
- **The supply chain.** `cargo deny` now rejects yanked crates, and two yanked versions in the lock (async-executor 1.14.1, clipboard-win 5.4.3) moved to current ones.
- **Smaller items.**
  - `SECURITY.md` is new.
  - The crate pages and crate docs describe 0.4.
  - The site has its favicon, and the web page's colour matches the app's panels.
  - The topic files and `CONTRIBUTING.md` were brought in line with the code, and every page with template code opens with a 0.4 version note.

Six findings were left: a henad-app test that reads a sibling fixture at run time, a `.typos.toml` leftover, `docs_rs.py` overriding `CARGO_TARGET_DIR`, henad-graph's `Field` and `Counter` having no constructors, a private doc example in the About window, and benchmark SVGs that no page uses.

### Dependencies

`cargo update` moved 165 locked packages, and rfd went from 0.17 to 0.18.
egui 0.37, egui_dock 0.22 and egui_plot 0.38 are out, but egui-snarl 0.12 requires egui 0.36, and a git dependency cannot be published.
Its port to egui 0.37 waits on two of its own dependencies.

Two patch releases change what users see, and the maintainer chose to take both and raise the floors to them:

- egui 0.36.2 honours a `TextEdit`'s minimum size, so the Editor's code field fills its view and a press below its last line focuses it.
- wgpu 30.0.2 marks the macOS window as sRGB, so a wide-gamut display shows the app's colours less saturated than before.

egui 0.36.2 also drops a value box's text when the value changed since the last frame.
The Parameters tab snaps a slider's value on every key, so typing 9951 into World Width one key at a time ended at 1.
The showcase renders found it, and every slider of the Parameters tab now applies its text on Enter or when the box loses focus.
The same test found the Sweep tab's range editor rounding a typed step of 0.05 to almost nothing, and its Minimum, Maximum and Step now commit the same way.
Each has a test that types one key per frame and fails without the fix.

### Card editor: the maintainer's eleven

The maintainer tried the sheet editor and listed eleven problems.
Three agents fixed them in parallel worktrees.

- **Values and types.**
  - A Calculate line can now take the parts of the lines above. The picker showed them but ignored a click.
  - The kinds read Boolean and Vector, and a line's messages begin "Line 3", the number the card shows.
  - Property rows have Edit and ×, or a ⋯ menu in a narrow pane, and the opened form scrolls into view.
  - A picker lists every value above its line and greys those that do not fit, with the checker's own reason, such as "Flockmates' centre is a vector, and Move to needs a cell."
  - The opening words of a random value open the slot's picker again, so a random number can go back to a typed one.
  - Type symbols stand before properties, parameters and picker items, and the View menu can hide them.
- **The world.**
  - The Colour selects show what is drawn and offer None where nothing would be.
  - A new agent model is coloured by heading from the start.
  - With a time unit set, the Playback counter and the Charts and Results axes read in the natural unit ("24 days", "1.1 years"), never shorter than the tick's own unit.
- **Drag and drop.**
  - A line moves by dragging its number, built on egui's own drag-and-drop calls.
    egui_dnd reorders a flat list with an animation, and the tree-view crates draw their own rows, which clash with the sentence controls.
  - A drop replays the line menu's moves as one Undo step, so ids and references behave as with the menu.
  - A drop the rules refuse shows red with its reason.

The new symbols and buttons made the Sheet pane's tables wrap words that fit before.
A table now drops its row buttons, then its symbols, before any word wraps.

### Card editor: the search and the second set

A read-only agent used the Sheet and Card panes as the maintainer had and found 15 more problems.
The maintainer chose these for the release:

- **Choices the build always refuses are greyed** in add menus, selects and boxes, with the build's message.
  The checker, the lowering and the menus refuse through one new module, `henad_graph::sheet::refusals`, and a test applies every greyed choice on five sheets to check that the build refuses each one with that message.
- **Edits that lose work ask first.** Deleting a property, parameter, state, event, button or line, retyping a property, replacing a painted start, and changing a colour rule with shades now list what will be lost, with line numbers.
  `SheetLosses` in henad-graph compares the sheet before and after, so an edit that loses nothing asks nothing.
- **Abilities the editor lacked.**
  - The measure form plots how many times an event happened.
  - A Choose row can be removed.
  - The line menu adds a line above or below, duplicates a line, and writes its note.
- **Smaller fixes.**
  - Every Sheet pane form scrolls into view.
  - A value inside a formula says how to change it instead of looking clickable.
  - Each bare "+" has a tooltip, and "including me" is a select.
  - The lowering's messages name items as the pane does.
  - Cmd+S, or Ctrl+S elsewhere, saves the model.
- **Status lines.** "Press Apply" no longer shows while Apply would refuse, and Apply is disabled on an empty sheet.
  The preview names Apply only when it runs scaled down, an Apply names the lines not run yet, and the Model tab names the sheet's preview while it runs.
- **New model.** The New grid rule and New node graph buttons are gone. The New model dialog holds Sheet, open by default, and Advanced, which holds Grid rule and Node graph.
  Opening a heading closes the one beside it.

The permission system refused one agent's `git commit`.
The maintainer approved committing its staged work as prepared, and the session committed it unchanged and checked the tree against the staged one.

### Screenshots

Six docs screenshots were taken again for the new UI, with their alt text.
The search for a fair retake of `overview.png` measured a GPU time per step 2.6 times the old shot's.
`henad-cli` timed `gpu_ants` at `46cb57d9`, where the old shot was taken, and at `113c0b9c`, alternating, and the newer commit was 2 to 5% faster.
The app's own overview scene at both commits reached the same tick in 40 seconds, within 0.2%.
The machine was slower that day, and the old shot stays.

### Edited tree

```
.github/workflows/ci.yml                     tidied
AGENTS.md                                    two new topic files in the table
CHANGELOG.md                                 0.4.0 dated, the review's corrections, the editor changes, the floors
Cargo.toml, Cargo.lock                       0.4.0, egui 0.36.2, wgpu and naga 30.0.2, rfd 0.18
SECURITY.md                                  new
deny.toml                                    yanked crates rejected
index.html                                   the page colour of the app's panels
agents/
├── henad-app-card-drag.md                   new: dragging lines, their slots, the drop's moves
├── henad-app-picker.md                      new: what each blank offers, greyed values, type symbols
└── ...                                      every area the changes touched
crates/
├── henad-app/src/
│   ├── tests/sheet_confirm.rs, sheet_drag.rs new, and tests in most existing sheet files
│   ├── ui/editor/sheet/card_drag.rs         new: drag and drop of card lines
│   ├── ui/editor/sheet/                     pickers, row buttons, symbols, questions, line menu, New model
│   ├── ui/params/, ui/sweep/                typed values commit on Enter or focus loss
│   └── ui/time_axis.rs, playback.rs         time in natural units
├── henad-graph/src/sheet/
│   ├── edit/losses.rs                       new: what an edit loses
│   ├── refusals.rs                          new: the rules the checker, the lowering and the menus share
│   ├── check/, lower/, sentence/            fit checks, named messages, the new words
│   └── tests/sheet_fit.rs, sheet_losses.rs, sheet_refusals.rs   new
└── */README.md, lib.rs                      the crate pages for 0.4
docs/
├── assets/app/                              the screenshots the changes show in
├── assets/benchmarks/                       record #111's figures and tables
├── developing/agent-record/20261126-112-…   this record
├── developing/releasing.md                  the release branch, the stability lists, the deprecation rule
├── guide/editor.md, app.md                  the editor's new behaviour
└── license.html                             regenerated
templates/model-project/                     0.4, the CC0 dedication, git init -b main
zensical.toml                                the favicon, this record's entry
```

## State after

`master` holds 57 commits after `ae5d5fad`, this record included.
The full local check passed on `113c0b9c` with 2,584 tests, as did both packaging passes, the pinned nightly's check, the params page and a publish dry run.
The release then follows the checklist in `releasing.md`: the `release-0.4.0` branch, its issue, the full tier on the branch, the signed tag, the publish and the GitHub release.

## Issues found & future directions

### Left, with the options for each

- **egui 0.37.** egui-snarl's port waits on egui-probe and egui-scale. Once it is out, egui 0.37, egui_dock 0.22 and egui_plot 0.38 can follow in a minor release.
- **The search's other four.** Sheet pane problems still show only in the list under the panes, not at their field.
  Parts of the starters cannot be written in the editor, result lines of some agent-only kinds show no name, and the optional "for each …" reads like an unfinished blank.
- **Drag and drop.** Whole cards do not move, and a When or Repeat cannot be dragged into another block. Either needs a new edit command.
  The line menu's Move up still takes a look line into a Repeat, which the drag refuses.
- **Type symbols** show in the Sheet pane only where the table fits at natural width, about 450 to 600 points of pane. Pickers always show them.
- **A Move line's mode.** Switching between by and to keeps the old value, which can then fail to fit until a new one is picked.
- **The History length box** applies every key, so typing 5000 resizes the history on the way.
- **`overview.png`** shows the removed New grid rule button in its Model tab, and needs a retake on a quiet machine.
- **Node graph starters** keep title case in their names, unlike the sheet starters above them in the dialog.
- **The light GPU runs of record #111's matrix** ran up to 28% slower than #48's, a per-step cost that the published cells do not show. The driver differed, and it was not looked into before the pod went.

### Not checked

- The macOS colour change on a wide-gamut display, by eye.
- The browser's own Cmd+S while the canvas has focus.
- The six skipped review findings above.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
