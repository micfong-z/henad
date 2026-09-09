---
date: 2026-09-09
title: "Getting a run out of the app"
description: The app grows an Export panel, the CSV writer moves down to henad-core so both front ends share one format, the chart history learns to keep a whole run, and an exported image is drawn again at the resolution of the layers rather than cropped out of the panel.
icon: material/application-export
status: ai-generated
model: claude-opus-5 (Claude Code)
issue: "#17"
state: implemented, `./check.sh` green, every export and every capture shape driven through the live app; the native save dialog itself is unverified
baseline_commit: 04fc86e
delta_state: uncommitted on `17-result-gui-export`
---

# Getting a run out of the app

> `henad-cli` could already write a run to disk three ways, and `henad-app` could write nothing at all.
> A run you watched was a run you could not keep, so analysing one meant re-running the configuration headlessly.
> The app now has an Export panel writing the stat series, a recording, the final state, the viewport and the run's details, all through one `rfd` save path that works on native and on the web.
> The CSV writer moved from the CLI binary down into henad-core, so the two front ends emit one format rather than two that drift.
> The issue named the obstacle itself: the chart history is a ring buffer, so an export off it would only ever return the tail of a run. It answers that twice, with an unlimited history setting and with a write-through recorder.

## State before

`crates/henad-cli/src/stats_export.rs` held `StatsWriter<W: Write>` with fifteen tests: column planning from the first sample, `.x`/`.y`/`.magnitude` for a vector, one column per bucket plus `.total` for a histogram, CSV escaping.
It was private to the `henad-cli` binary, so nothing else could reach it.
`write_state` in `main.rs` formatted the final state inline and had no tests.

`henad-app` contained no file IO at all: no `std::fs`, no save, no export, no `rfd`.
Its only `web_sys` use was the canvas lookup in `main.rs`.

`StatsHistory` stored one `f64` per series per sample, flattened through `StatValue::scalar()` at the push in `lib.rs` before the history ever saw the value.
A vector stat therefore reached it as a magnitude and a histogram as a total, both unrecoverable.
Its capacity was a plain `usize` with a 100..=1_000_000 slider, and the oldest samples were overwritten once it filled.

## What was done

New files marked **+**, modified marked **~**, moved marked **→**.

```
CHANGELOG.md                                 ~ Unreleased/Added
Cargo.toml                                   ~ rfd 0.17
docs/license.html                            ~ regenerated (rfd, dispatch2)

crates/henad-core/
└── src/
    ├── lib.rs                               ~ pub mod export
    ├── view.rs                              ~ StatsHistory: unlimited, full values, entries()
    └── export/
        ├── mod.rs                           + re-exports
        ├── stats_csv.rs                     → from henad-cli, anyhow dropped, into_inner()
        └── state.rs                         + write_grid, write_points, lifted off SimState

crates/henad-cli/
└── src/
    ├── main.rs                              ~ delegates both formats to henad_core::export
    └── stats_export.rs                      → moved out

crates/henad-app/
├── Cargo.toml                               ~ rfd, serde_json
└── src/
    ├── lib.rs                               ~ recording, save and capture polling
    ├── state.rs                             ~ recording, save channel, capture, history_capacity
    └── ui/
        ├── mod.rs                           ~ pub mod export
        ├── agent_layer.rs                   ~ AgentDraw::record, offscreen_draw
        ├── charts.rs                        ~ Unlimited history control
        ├── dock.rs                          ~ Tab::Export, layout, generalised tests
        └── export/
            ├── mod.rs                       + the panel and Recording
            ├── image.rs                     + offscreen capture at the layers' resolution
            ├── save.rs                      + spawn_save, one arm per target
            └── metadata.rs                  + run details as JSON

docs/
├── guide/app.md                             ~ Export tab section, Unlimited history
├── reference/cli.md                         ~ points at the app's twin
└── developing/agent-record/…-27-…md         + this record

zensical.toml                                ~ nav entry
```

**The CSV writer moved down.**
CLI plus GUI is the two callers AGENTS.md asks for before anything is extracted.
henad-core has no dependencies, so `anyhow` was replaced by a `StatsWriteError` with exactly the two failure modes the writer has, an IO error and a shape change, carrying `Display`, `Error` and `From<io::Error>` so the CLI's `?` still works through `anyhow`.
The `W: Write` bound stayed: the CLI keeps streaming into a `BufWriter<File>` and flat memory, the app builds a `Vec<u8>` for the dialog.
`into_inner` was added for the app, which owns its destination; `finish` is now a thin wrapper over it.

**The final-state format was lifted off `SimState`.**
The CLI reaches it through `grid_view()`/`point_view()`, the app through the snapshot's `CpuLayers`, so `write_grid` and `write_points` take slices and each caller adapts. The bytes are unchanged, and the format gained the two tests it never had.

**`StatsHistory` keeps what a scalar cannot round-trip.**
`capacity` became `Option<usize>`, `None` retaining everything; `Vec::with_capacity` only pre-allocates in the bounded arm, and the ring arithmetic moved into one `buf_index` helper the readers share.
A second store, `full: Vec<Option<Vec<StatValue>>>`, holds full values for the series that need them, decided on the first sample the way the CSV layout is.
`StatValue` is 56 bytes against an `f64`'s 8, so storing everything that way would have cost seven times the memory for data the charts never read; only boids and gpu_boids have a non-scalar stat, and every other model pays nothing.
`push` became `push_entries`, taking `&[StatEntry]` so the history sees the value before it is flattened, and `entries(j)` rebuilds a sample in the shape `StatsWriter` takes.
The charts' `get(col, j)` read path is untouched.

**Recording is the write-through twin.**
It feeds a `StatsWriter<Vec<u8>>` from the snapshot poll, so its columns are whatever the model declares and its memory is the CSV text rather than a structured sample per tick.
It survives Offload and a rebuild: both now close a running recording off instead of dropping it, since freeing a simulation should not throw away captured rows.

**One save path for both targets.**
`rfd` 0.17 defaults to xdg-portal plus wayland rather than gtk3, so the Linux CI job needs no new packages, and its wasm backend turns `save_file` into a browser download.
`ui/export/save.rs` follows `painted.rs`'s idiom, one same-signature `spawn` per `cfg`: a thread plus `pollster` on native, `spawn_local` on the web.
Outcomes return over a `flume` channel drained each frame.
An export failure reports into the panel, never through `report_fault`, which calls `offload_simulation` and would tear the simulation down over a failed file write.

**The viewport capture draws again rather than photographing the panel.**
The first attempt was `ViewportCommand::Screenshot` cropped to the viewport rect, which is implemented for wgpu on both targets and is wrong: the panel fits the layers to whatever rect it has, so a 1024 cell grid squeezed into an 800 point panel came back as moire.

`ui/export/image.rs` renders the layers into an offscreen target sized from the data, through the same pipelines the panel uses. A CPU grid is written straight in with `write_texture`, since it has no pipeline of its own; a GPU display runs its existing fullscreen-triangle pass; the population is drawn over the top. `AgentDraw::record` came out of `CallbackTrait::paint` so one body serves egui's pass and this one.

Resolution comes from the layers. One pixel per cell for a grid. A model with agents grows its field by a whole number of times until the long side reaches `MIN_AGENT_DIM`, because a field is coarse next to the population over it and rounding 50 000 ants into a 201 pixel image loses every sub-cell position. Whole numbers keep each cell a square block under the nearest sampling both display paths already use. Sprites are three pixels, as on screen.

Readback follows `CounterReadback`: `map_async` right after submission, polled every frame, never blocking, since the web cannot block on the main thread. Rows are padded to `COPY_BYTES_PER_ROW_ALIGNMENT` and the surface's BGRA order is swapped back on the way out.

**Four things came back out again.**
The panel wrapped itself in a `ScrollArea`, which `egui_dock` already does for a tab's contents, so a tab that overflows scrolls twice.
Its five headings went through a `heading(ui, icon, text)` wrapper for an icon each; both went, and the headings are a bare `ui.strong`.
`image_name` was a named call to `file_name(app, "viewport", "png")` with one caller.
`Recording::is_running` was written and never called.

A sweep for more of the same found `to_rgba8` and `from_rgba8`: two functions with identical bodies, named for the direction each was used in, over an `is_bgra` and a `swap_rb`. Swapping red and blue is its own inverse, so the four are one `match_channel_order`.

`make_target`, `write_grid` and `draw_layers` have one caller each and stayed. They name the three phases of a submission and keep `start` under `clippy::too_many_lines`, which is a different thing from wrapping a call in a name.

## State after

`./check.sh` is green, the web build included, which is where the `rfd` dependency was most at risk.
`docs/license.html` was regenerated: `rfd` and `dispatch2`, both MIT, both already on `deny.toml`'s allow list.

Twelve new tests.
Seven on `StatsHistory`: bounded and unlimited retention, a vector series keeping its components while the chart still reads a magnitude, a scalar series rebuilding from its own column, full values wrapping in step with their scalars, resize in both directions, and a vector series costing more heap than a scalar one.
Two on the state format.
Two on the seam that matters, under `export::stats_csv::tests::parity`: the same samples pushed into a recording and into a history must render the same CSV, and a wrapped history must render the tail of the recording with an identical header.
`ui/dock.rs`'s layout tests were generalised rather than renumbered — a `STACKED` const now names the pairs the default layout stacks on purpose, so the next tab added fails the test for a reason worth reading.

Driven through the live app over the egui inspection port: the panel renders, the sample count and tick span track the run, and every export builds its bytes and opens a dialog with the right filename.

The capture was checked on one model of each shape, reading the resolution the panel reports and then taking it:

- SIR, a CPU grid with nothing over it, 1024 x 1024, one pixel per cell,
- boids, a CPU population with no field, 1000 x 1000, one pixel per world unit,
- ants, a CPU population over a field, 1005 x 1005, its 201 cell field five pixels to a cell,
- ant foraging on the GPU, 1000 x 1000 from a 200 cell field, with Export state correctly refused,
- Game of Life on the GPU, 1024 x 1024, no growth since it carries no agents.

## Issues found & future directions

1. **The save dialog itself is unverified, and cannot be verified from here.**
   A `cargo run` binary has no bundle identifier, so computer-use cannot reach a native `NSSavePanel`, and the dialogs opened during testing were never completed.
   Everything up to the dialog is covered — the bytes by tests, the dispatch and the resolution by the panel's own readout — but `rfd`'s write call has not been watched to land a file, so no exported PNG has been looked at.
   A bundled `.app` would be drivable.
2. **A save panel can open behind the app window.**
   That is what the four unactioned dialogs turned out to be, and the status line read `Saving …` throughout, which looks like a stall rather than something waiting on the user.
   The wording is now `Choose where to save …`. A modal-style hint in the panel would be better still.
3. **Only one finished recording is held at a time.**
   Stopping a second one replaces the first, unsaved, without warning.
4. **The Export panel is tall, and where it sits decides whether that shows.**
   It went behind Pacing in the bottom-left first, where five sections did not fit and the buttons clipped until the panel was enlarged.
   Behind Charts it has the height and the width for all five, and `egui_dock`'s own scroll covers a window small enough to lose them again.
5. **`MIN_AGENT_DIM` is a number, not a decision the user makes.**
   A thousand pixels suits the four agent models in the tree and nothing else was measured against it.
   A resolution field in the panel was the alternative, and is what a user wanting a poster or a thumbnail would reach for.
6. **A GPU model cannot export its final state.**
   `GpuSimState` has a stats readback and no buffer readback, so the button is disabled with a reason. `henad-cli --export` is CPU-only for the same reason, so this is parity rather than a gap, and one readback path would fix both.
7. **The app still passes no seed.**
   `state.rs` hands `None` to the factory, so a run uses the model's fixed default. That is reproducible, which is why the run-details JSON is worth anything, but a settable seed is what would make that file round-trip into a CLI invocation.
8. **`serde` is declared in henad-app and used by nothing.**
   `serde_json` is now a real dependency, `serde` is still the leftover from `eframe_template` it always was.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

- Started this session independently
- Edited and rewrote the majority of GUI generated
- Reverted unnecessary refactors
- Corrected LLM understanding of `egui_dock`
- Rewrote relavent documentation
- Redirected LLM 5 times in other scope issues
