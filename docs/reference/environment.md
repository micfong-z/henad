---
title: Environment variables
description: The three environment variables the Henad engine reads.
icon: material/variable
---

# Environment variables

`HENAD_REQUIRE_GPU=1`

:   Turns "no adapter on this machine" from a silent test skip into a failure.
    CI sets it on Linux, macOS and Windows.
    Run the GPU tests with it set before treating them as passing.

`HENAD_DUMP_WGSL=<dir>`

:   Writes every shader the engine compiles to `<dir>/<label>.wgsl`.
    Shared WGSL reaches the compiler through `#import` and resolves at build time, and a dumped file therefore shows the assembled source behind a validation error.

`EGUI_INSPECTION=1`

:   With `--features inspection`, opens the app's inspection port on 5719.
    The port exposes the live widget tree.
