---
date: 2026-09-08
title: "An About menu, and the build stamp behind it"
description: A second menu bar entry links out to the repository and the docs site, and opens a window naming the version, the commit and the licence.
icon: material/note-text-outline
status: ai-generated
model: claude-opus-5 (Claude Code)
issue: none
state: implemented and driven through the live app, `./check.sh` green
baseline_commit: 0fd5051
delta_state: uncommitted on `master`
---

# An About menu, and the build stamp behind it

> The menu bar had one entry, View, and nothing anywhere named the running build.
> There is now an About menu with three items: two links out to the repository and the documentation site, and a window carrying the logo, the version, the commit, whether the binary is a release build, and the licence.
> `build.rs` stamps the commit and its date into the binary as environment variables, so the window reports the tree it was built from rather than a hard-coded string.
> The two menu links needed eframe's `links` feature, which the workspace had off. Without it `open_url` is silently a no-op on native.

## State before

`menu_bar.rs` held a single `view_menu`, and `Tab::ALL` was the only thing it offered.
Nothing in the app named its own version, its commit, or its licence, so a bug report had no build to attach to it.
The logo assets landed under `assets/` earlier the same day and nothing referenced them.

`eframe` was pinned with `default-features = false` and four features, none of them `links`.
`egui-winit` compiles `open_url_in_browser` to a `log::warn!` without it, so any hyperlink or `Context::open_url` on native did nothing.

## What was done

New files marked **+**, modified marked **~**.

```
Cargo.toml                                   ~ eframe gains "links", and `image` joins the
                                               workspace deps (eframe already pulls the same
                                               crate with the same single feature)

crates/henad-app/
├── Cargo.toml                               ~ image
├── build.rs                                 ~ emit_commit_stamp(): HENAD_COMMIT and
│                                               HENAD_COMMIT_DATE via `git`, plus rerun-if-changed
│                                               on HEAD and the branch ref
└── src/
    ├── lib.rs                               ~ menu_bar_panel now takes the state; about_modal
    │                                          runs beside fault_modal
    ├── state.rs                             ~ about_open, logo_texture
    └── ui/
        ├── mod.rs                           ~ registers about
        ├── menu_bar.rs                      ~ about_menu + link_button
        └── about.rs                         + the window, the build rows, and the clipboard text
```

### The menu

`about_menu` sits after `view_menu` and holds Source code, Documentation and About Henad.
The two links carry `MDI_OPEN_IN_NEW` and show the destination on hover, and go through `Context::open_url` so the web build opens a tab and the native build hands off to the system browser.

### The window

A `Modal`, the same container `fault.rs` uses. The logo, the name and the tagline sit in a row, with the two links repeated underneath the tagline; a two-column grid below carries Version, Commit, Build and License; Copy and Close sit under a separator.

Four things the layout has to survive:

- **A 300pt window.** `main.rs` sets that as the minimum, well under the 420pt the window wants. The width is pinned with `set_width` to whatever fits, the area is `constrain`ed, and the tagline wraps. Capping with `set_max_width` instead let the window size itself past a narrow viewport and put the label column off the left edge.
- **A short window.** The header and the grid scroll inside a `ScrollArea`, so Copy and Close stay reachable.
- **No git.** `HENAD_COMMIT` comes out empty from a source tarball, and the row then reads `Unknown`.
- **The clipboard.** Copy writes one `Label: value` line per row. It is plain ASCII on purpose: the app's clipboard path mangles non-ASCII on macOS, and a `·` separator arrived as a single `0xe1` byte.

`Build` reads Release or Debug from `cfg!(debug_assertions)`.
Benchmark numbers here have been wrong before because a debug build was mistaken for a release one, so the window says which it is.

### The logo

`assets/henad-logo-transparent-256.png` is included in the binary and decoded once on the first open through `image`, then kept in `AppState::logo_texture`.
`reset_simulation` clears the grid and density textures and leaves this one alone.
The white mark suits the dark window fill, which is the only fill the app has: `setup_custom_styles` gives the light theme slot the dark style too.

## State after

`./check.sh` is green, the web build included.
Two tests cover the parts that can rot without anyone noticing: the bundled PNG decodes to a square, and Copy emits a line per row.

Driven through the live app over the egui inspection port:

- the three menu items render with their icons,
- About Henad opens the window and Close, Escape and a backdrop click all close it,
- Copy puts four clean-ASCII lines on the clipboard,
- Documentation brings the browser to the front, which is what the `links` feature buys,
- the window fits inside a 340 by 400 viewport with every label and both buttons visible.

The commit row reads `0fd5051d (2026-09-08)` on this tree.

## Issues found & future directions

1. **Popups and modals are translucent.**
   The View menu, the About menu and both modals let the panels behind them show through, badly enough that the About grid is hard to read over the viewport.
   This predates the session, `Frame::popup` uses `window_fill()` and the style sets that opaque, so the cause is somewhere else.
   The View menu shows it on an untouched code path, which is the cheapest place to reproduce it.
2. **The clipboard is not UTF-8 clean on macOS.**
   `Context::copy_text` turned `·` into a lone `0xe1`, the Mac OS Roman byte for it.
   Avoided here by staying ASCII. It still affects the fault modal's Copy, where an error message is not under our control.
3. **`links` costs 32 crates.**
   `webbrowser` pulls `url`, `idna` and the `icu` tree, and `jni` for Android.
   The alternative is spawning `open`/`xdg-open`/`start` directly, which is a dozen lines but puts the platform matrix, the missing-opener case and process reaping on us.
4. **The window is native-shaped in one place.**
   `Build` and the commit are the same on the web, but a wasm visitor has no notion of a local build.
   A `Target` row, or the host summary the System tab already collects, would say more there.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

A fairly trivial task.
Manually changed a few stylistic/conventional points.
