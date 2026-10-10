---
title: Editor
description: Writing a grid rule in the app's Editor tab, and running it without a restart.
icon: material/code-braces
---

# Editor

The <span class="ui" markdown>:material-code-braces: Editor</span> tab edits a *grid rule*, a GPU grid model written as data, and runs it in the app without a restart.
A grid rule lives in a *model file*, one TOML file that holds the rule's states, parameters and grid, and its step in WGSL.
The app compiles the step while it runs, in the official app and in a browser alike, and the rule runs on the same GPU engine as the compiled GPU models.

!!! info "Henad 0.4"

    This page describes Henad 0.4.

## Opening the Editor

The Editor is closed in the default layout.
It opens in a panel of its own to the right of the <span class="ui" markdown>:material-cube-outline: Viewport</span>, so the run stays in view while you edit its code.

- Press <span class="ui" markdown>:material-file-plus-outline: New grid rule</span> in the <span class="ui" markdown>:material-cog-outline: Model</span> tab to open the Editor on the Life starter, Conway's Game of Life written as a grid rule.
- Press <span class="ui" markdown>:material-code-braces: Edit model</span> in the Model tab to open the Editor on the model file of the selected grid rule.
  The button appears only for a model applied from the Editor.
- In the menu bar, <span class="ui" markdown>:material-file-outline: File</span> holds New grid rule, Open model and Save model, and <span class="ui" markdown>:material-view-dashboard-outline: View</span> toggles the Editor like any other tab.

New grid rule and Open model replace the Editor's text.
Edit model replaces it too, but keeps the text, your edits since the last Apply included, while the selected rule runs the code the Editor applied last.
Save what you want to keep first.

## The model file

The Life starter is this file, without its comments:

```toml title="life_rule.toml"
--8<-- "crates/henad-explore/specs/models/life_rule.toml"
```

The header names the format, its version and the kind of model, then the rule's `id`, `name` and `description`.
The `id` is the model's id in the app, and a rule whose id is already in the app replaces that model, a built-in one included.

`[grid]`
: The default `width` and `height` of the grid, and its `boundary`, `torus` or `bounded`.

`[[state]]`
: One table per state a cell can take, numbered from 0 in the order they appear.
  Each has a `label` and an RGBA `color`.
  An `initial` names the `f32` parameter that gives the state's share of the cells at tick 0, and the one state without an `initial` takes the cells left over.

`[[param]]`
: One table per parameter, with the keys `henad-cli --params --json` prints: `id`, `label`, `kind` (`f32`, `u32`, `bool` or `choice`), `min`, `max`, `default`, and an `f32`'s `step`.
  The app puts **Grid Width** and **Grid Height** in front of them.
  Every parameter of a grid rule applies when the model is built.

`[step]`
: The step, between `'''` and `'''`.
  It defines `fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32`, which returns the next state of one cell.

A step reads its neighbourhood through a few names that the engine declares in front of it:

| Name | What it gives |
|---|---|
| `neighbor(dx, dy)` | State of the cell `dx` columns and `dy` rows away, or 0 past the edge of a bounded grid |
| `moore_count(state)` | Number of the eight surrounding cells in `state` |
| `von_neumann_count(state)` | Number of the four adjacent cells in `state` |
| `NUM_STATES` | Number of states |
| `DEAD`, `ALIVE`, ... | One constant per state, its label in upper case with spaces as underscores |
| `params.density`, ... | Value of each parameter, by its id |

`rng` holds a random word for the cell and the tick.
Import the shared random functions to draw from it, as in `#import henad::rng::{next_float}` and `next_float(rng, 1.0)`.
A step that returns a state past the last one leaves the cell in the last state.

## Apply

Press <span class="ui" markdown>:material-check: Apply</span> to check the text and install the rule.
The check reads the file, then compiles the step.
Nothing runs until the check passes.

When it passes, the rule joins the app's models, or replaces the model with its id, and becomes the selected model.
What happens to the run depends on what was running:

- With no run, a run still at tick 0, or a model still building, the rule is built at once and the run starts at tick 0.
  The desktop app builds it on a thread of its own, as [Building a model](app.md#building-a-model) describes, and the Editor reads **Model applied.** from the start of the build.
  Press <span class="ui">Cancel</span> in the Playback tab to stop the build, and the Editor reads **Model applied. Press Build to run it.**
- A run of the same rule past tick 0 opens the **Rebuild model** dialog, which reads **The model will be rebuilt from tick 0. The current run will be lost.**
  The Editor reads **Model applied. Press Rebuild to run it from tick 0.**
  Press <span class="ui">Rebuild</span> to start the new rule from tick 0, or <span class="ui">Cancel</span> to keep the run going under the code it was built with.
  After Cancel, <span class="ui" markdown>:material-restart: Build</span> runs the new rule, and Apply opens the dialog again.
  The app tour's [Model code changes](app.md#model-code-changes) describes the dialog.
- A run of another model past tick 0 keeps going.
  The Editor reads **Model applied. Press Build to run it.**, and <span class="ui" markdown>:material-restart: Build</span> runs the rule.

Applying the code the run already runs changes nothing, and the Editor reads **Model code unchanged.**

## Problems

The list under the code shows the outcome of the last Apply.
A text that fails the check leaves the running model alone, and the list names each problem at its line and column, as in **37:5 loops are not allowed in a grid rule. Without them a step's work is bounded** for a `loop {}` on the line after `let alive` in the Life starter.
The line numbers are those of the gutter beside the code.
Press a problem to move the cursor to it.
The list holds the problems of the last Apply, and an edit since then shows its problems on the next one.

The check refuses a step that could run without end or reach past its own cell:

- loops: `loop`, `for` and `while`,
- a chain of function calls past 65,536 expressions and statements for one cell,
- bindings, entry points of its own, atomics and barriers,
- writes to any memory but the step's own variables, and reads of the engine's buffers but `params`,
- a name the engine declares, declared again,
- variables past the 8,192-byte limits of WGSL, and a step past 64 KiB.

The device can still refuse a rule that the check passed, as a browser's WebGPU can.
When it refuses the rule as the rule is built, before the run passes tick 0, the Editor reads **Model build failed. Fix the problem below.** over the device's message, and the run is gone.
The message appears a moment after the build starts, in place of **Model applied.**
Fix the text and press Apply to build it again.
A fault that no edit of the text can fix, such as a lost device or a device out of memory, opens the app's fault dialog instead.

## Saving and opening model files

Press <span class="ui" markdown>:material-tray-arrow-down: Save model</span> to save the Editor's text as written, comments included.
The desktop app asks where to save, and suggests the name of the file you opened or the rule's id.
A browser downloads the file.

Press <span class="ui" markdown>:material-folder-open-outline: Open model</span> to read a `.toml` model file into the Editor.
Opening a file does not apply it.
Press Apply to install it.

A model file holds data and WGSL alone, and its step runs only on the GPU, inside the checks above and the device's own.
It is safe to share.

## Limits

- The Editor writes grid rules alone.
  Agent models and CPU models cannot be written as a model file.
- A step has no initialisation of its own.
  The states' `initial` shares seed the grid at tick 0.
- A grid rule reads its parameters when it is built.
  An edited parameter applies after <span class="ui" markdown>:material-restart: Build</span>.
- The Editor has no syntax highlighting, and checks the text only when you press Apply.
- The text is not kept when the app closes.
  Save it first.
- The shaders of the compiled GPU models cannot be edited here.
  A [dev host](hot-reload.md) reloads the models of your own project.
- A results folder of a grid rule cannot rebuild the rule in the app.
- A grid rule needs a GPU with compute support.
  On a device without it, the Editor reads **Grid rules cannot run on this device. They need a GPU with compute support.**

*[GPU]: Graphics processing unit
*[CPU]: Central processing unit
*[WGSL]: WebGPU Shading Language
*[TOML]: Tom's Obvious Minimal Language
