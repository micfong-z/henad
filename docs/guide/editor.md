---
title: Editor
description: Writing a grid rule or a graph model in the app's Editor tab, previewing it, and running it without a restart.
icon: material/code-braces
---

# Editor

The <span class="ui" markdown>:material-code-braces: Editor</span> tab edits a model file and runs its model in the app without a restart.
A *model file* is one TOML file, and it holds one of two kinds of model.
A *grid rule* is a GPU grid model written as data, with its step in WGSL, and the Editor shows it as text.
A *graph model* is built from nodes on a canvas, and the Editor shows it as [a graph](#graph-models) with a live preview beside it.
The app compiles either kind while it runs, in the official app and in a browser alike, and the model runs on the same GPU engine as the compiled GPU models.

!!! info "Henad 0.4"

    This page describes Henad 0.4.

## Opening the Editor

The Editor is closed in the default layout.
It opens in a panel of its own to the right of the <span class="ui" markdown>:material-cube-outline: Viewport</span>, so the run stays in view while you edit its code.

- Press <span class="ui" markdown>:material-file-plus-outline: New grid rule</span> in the <span class="ui" markdown>:material-cog-outline: Model</span> tab to open the Editor on the Life starter, Conway's Game of Life written as a grid rule.
- Press <span class="ui" markdown>:material-code-braces: Edit model</span> in the Model tab to open the Editor on the model file of the selected grid rule.
  The button appears only for a model applied from the Editor.
- Press <span class="ui" markdown>:material-file-document-plus-outline: New model</span> in the Editor or the Model tab to start a sheet, a model described in sentences, from Game of Life, Majority vote or Forest fire.
  The [app guide](app.md#sheets) describes the sheet.
- Press <span class="ui" markdown>:material-graph-outline: New node graph</span> in the Editor and pick an example to open it as a graph model: Game of Life, Vote or SIR Epidemic on a grid, or Boids Flocking, Ant Foraging or Random Walk of agents.
- In the menu bar, <span class="ui" markdown>:material-file-outline: File</span> holds New model, New grid rule, New node graph, Open model and Save model, and <span class="ui" markdown>:material-view-dashboard-outline: View</span> toggles the Editor like any other tab.

New grid rule and Open model replace the Editor's text.
Edit model replaces it too, but keeps the text, your edits since the last Apply included, while the selected rule runs the code the Editor applied last.
Save what you want to keep first.

## The model file

The Life starter is this file without the comments outside its step:

```toml title="life_rule.toml"
--8<-- "crates/henad-explore/specs/models/life_rule.toml"
```

The header names the format, its version and the kind of model, then the rule's `id`, `name` and `description`.
The `id` is the model's id in the app, and a rule whose id is already in the app replaces that model, a built-in one included.

The tables declare the grid, the states and the parameters, and `[step]` holds the step between `'''` and `'''`.
The step defines `fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32`, which returns the next state of one cell.
[Model files](../reference/model-files.md#the-format) lists the keys of each table.
[What the template declares](../reference/model-files.md#what-the-template-declares) lists the names a step reads its neighbours and parameters through, and [Randomness](../reference/model-files.md#randomness) describes how a step draws from `rng`.

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
A text that fails the check leaves the running model alone, and the list names each problem at its line and column, as in **37:5 loops are not allowed in a grid rule. Step functions must be bounded** for a `loop {}` on the line after `let alive` in the Life starter.
The line numbers are those of the gutter beside the code.
Press a problem to move the cursor to it.
The list holds the problems of the last Apply, and an edit since then shows its problems on the next one.

The check refuses a step that could run without end or reach past its own cell.
[What is refused](../reference/model-files.md#what-is-refused) lists each refusal, and [Limits](../reference/model-files.md#limits) the sizes a step has to keep within.

The device can still refuse a rule that the check passed, as a browser's WebGPU can.
When it refuses the rule as the rule is built, before the run passes tick 0, the Editor reads **Model build failed. Fix the problem below.** over the device's message, and the run is gone.
The message appears a moment after the build starts, in place of **Model applied.**
Fix the text and press Apply to build it again.
A fault that no edit of the text can fix, such as a lost device or a device out of memory, opens the app's fault dialog instead.

## Graph models

A graph model opens on a canvas.
The panel beside it lists the model's passes by stage, init, tick, stats, view and action, then its functions, and under them the world: the grid's size and edges, or the number of agents and the world's size, and the parameters, lanes, categories, statistics and actions.
Press a pass to open it on the canvas.
A tick pass runs once per tick for every cell or agent, an init pass seeds the cells or agents when the model is built, and an action pass runs when its action is pressed.

On the canvas, a node computes a value from its inputs, and a wire carries a value from an output to an input.
Right-click empty canvas to add a node, or drop a wire on empty canvas to add a node that takes it.
Right-click a node's header for its menu: Frame, Copy, Inputs (bind an input to a parameter, set a constant or clear it), Settings, View in viewport, Move into region, Move out of region and Delete.
A click selects a node, <kbd>Delete</kbd> removes the selection, and <kbd>Cmd</kbd>+<kbd>Z</kbd> (<kbd>Ctrl</kbd>+<kbd>Z</kbd> outside macOS) undoes an edit.
The toolbar above the canvas holds Undo, Redo, Copy, Delete, Repeat, Function, Arrange and Frame all.

### The preview

The Editor runs the graph as a *preview* in the Viewport while you edit it.
A preview is the model at a small size: each side of the grid at most 256 cells, or at most 16384 agents in a world of the model's own size, with every parameter at its default.
After each edit that leaves the graph valid, the preview builds again and starts from tick 0 once you pause for a moment.
Moving a node, or pressing Arrange, changes no code and leaves the preview running.
The list under the canvas reads **Preview runs at 256 × 256 cells from tick 0. Press Apply to run the model at its own size.**, or the number of agents, and the Playback tab names the preview.
The sizes in the world panel and the values in the Parameters tab are left as they are.

The preview takes the Viewport only when nothing runs, when the run is still at tick 0, or when the run is a preview.
A run past tick 0 keeps going, and the Editor reads **Preview waits for the running model.** until the run ends.
Pressing <span class="ui" markdown>:material-restart: Build</span> after the edit keeps the Viewport for that run.

An edit that leaves the graph invalid leaves the preview running.
The node with the problem takes a red frame and a badge, and the list under the canvas names the problem with its pass and node, as in **step, n16: Greater n16: the input `b` is f32, and the node needs u32**.
Press the badge, or the problem in the list, to bring the node into view.
A pass with problems shows their number beside its name.

Apply runs the graph at its own size, as for a grid rule, and replaces a preview at any tick.
A model too heavy for the device at its own size is not built, and the Editor reads **Model build failed. Fix the problem below.** over the work it would need.
The preview of the same model can still run, since it is smaller.

### Regions and Repeat

A *region* is a node whose body runs many times for each cell or agent, and whose results are gathered into *accumulators*.
**Repeat** runs its body a counted number of times, a neighbourhood fold runs it once for each neighbouring cell, and a radius fold once for each agent within a radius.
A region draws as two nodes inside a frame: its input half on the left holds its inputs and the values its body reads, such as `k`, the number of the repetition from 0, and its output half on the right holds its accumulators.
The nodes of its body sit inside the frame between the halves.
Each Accumulate node inside the body feeds one accumulator, and the accumulator's rule combines the values: `sum`, `count`, `min`, `max`, `or`, `and`, or `carry`, which keeps the last value and lets the body read it again as `prev.<name>` on the next repetition.

Select nodes and press <span class="ui" markdown>:material-repeat: Repeat</span> to wrap them in a new Repeat that runs once.
The model computes what it did before, and each value the wrapped nodes passed to a node outside becomes a `carry` accumulator.
A selection that holds a node a region body cannot hold, such as a lane write, or an Accumulate node without its region, does not wrap.
Set the Repeat's `count` to a constant, or bind it to a parameter through the node menu's Inputs, and set <span class="ui">Max</span> on its input half, the most repetitions it runs.
A constant count sets Max for you, and a count that can pass Max shows a warning on the node.
<span class="ui">Add accumulator</span> on the input half adds an accumulator, whose name, type and rule each have a field beside it.

Drag a node into a region's frame to move it into the region's body, and use the node menu's Move out of region to take it out again.
A node taken out of a region moves to the right of the region's frame, so it no longer looks like a member.
The frame a node would join takes a blue outline while you drag it.
Right-clicking empty canvas inside a frame adds a node to that region.
Regions nest at most three deep, a Repeat inside a Repeat or a neighbourhood fold and the other way round, and a radius fold neither holds a region nor sits in one.
The menus and the frames offer only the moves that follow these rules.

### Functions

A *function* is a named graph that passes can call, each call a node with the function's inputs and outputs.
The functions panel lists the model's functions.
Press a function to open its body on the canvas, or right-click a call and press Open function, and the toolbar shows where you came from, as in **flock › clamp_speed function**.
Press a name in that breadcrumb to go back.
Inside a function, a Function input node reads one of its inputs and a Function output node sets one of its outputs.
The panel renames the open function, adds, renames, retypes and removes its inputs and outputs, and removes it.
An edit inside a function changes every call, and the preview restarts as for any edit.

Select nodes and press <span class="ui" markdown>:material-function-variant: Function</span> to move them into a new function, named `function` or `function_2` and so on, with a call in their place.
Each value they read from outside becomes an input, and each value a node outside reads becomes an output.
Right-click a call and press Ungroup to put a copy of its function's nodes back in the call's place.
The function stays in the panel until you remove it.
A random draw inside a function draws other numbers for each call, so a selection that draws numbers draws different ones once it becomes a function.
Ungroup keeps the numbers the call drew.

Press <span class="ui" markdown>:material-file-import-outline: Import function</span> and pick another model file to list its functions.
Press Import beside one to copy it, and every function it calls, into this model, under the name in the field beside it.
A name the model already uses, or the name of a function the imported one calls, is refused, so type another one.
The model file records the name of the file each imported function came from.
An import copies the function alone, so a function that reads a lane or a category of the other model shows a problem until this model declares one with the same id.

### Expressions

An Expression node computes its outputs from a short formula, such as `out = if(count > 0, moved + avg * inv, moved)`.
Its body holds the formula, its named inputs with their types, and its outputs, separated by commas.
Each change applies when the field loses focus, and a formula that does not read shows its problem on the node.
Renaming an input keeps the wire it held.

### Generated WGSL and View in viewport

Press <span class="ui" markdown>:material-code-braces: WGSL</span> in the canvas toolbar to show the WGSL compiled for the open pass, read-only, beside the canvas.
Each value is named after its node, so `n13_out` is the output of node n13.
An init pass runs on the host when the model is built and has no WGSL.

Right-click a node and pick an output under **View in viewport** to colour the preview by that value, one cell at a time.
A number goes through a colour ramp, a flag shows as the ramp's two ends, and a category shows in its own colours.
The toolbar reads **Viewing n11.count of pass step**, and <span class="ui" markdown>:material-eye-off-outline: Stop viewing</span> returns to the model's own colours.
Viewing changes nothing in the model file.
The value is computed from the cells as they are after each tick, so a value that reads a random draw cannot be viewed.

## Saving and opening model files

Press <span class="ui" markdown>:material-tray-arrow-down: Save model</span> to save the Editor's text as written, comments included.
A graph model is saved in its canonical form, with the canvas layout in its `[editor]` table.
The desktop app asks where to save, and suggests the name of the file you opened or the rule's id.
A browser downloads the file.

Press <span class="ui" markdown>:material-folder-open-outline: Open model</span> to read a `.toml` model file into the Editor.
A graph model opens on the canvas, and any other file opens as text.
Opening a file does not apply it.
Press Apply to install it.

A model file holds data and WGSL alone, and its step runs only on the GPU, inside the checks above and the device's own.
It runs no native code.
A heavy step on a large grid can still take long enough per tick to lose the GPU device, as the [model file reference](../reference/model-files.md#limits) describes.

## Limits

- The Editor writes grid rules, and graph models on a grid or of agents.
  CPU models cannot be written as a model file.
- A step has no initialisation of its own.
  The states' `initial` shares seed the grid at tick 0.
- A grid rule or a graph model reads its parameters when it is built.
  An edited parameter applies after <span class="ui" markdown>:material-restart: Build</span>.
- The Editor has no syntax highlighting, and checks the text only when you press Apply.
- The text is not kept when the app closes.
  Save it first.
- The shaders of the compiled GPU models cannot be edited here.
- A results folder of a grid rule cannot rebuild the rule in the app.
- A GPU model runs many ticks per frame, so a small preview can reach the end of its run within a second.
- A grid rule or a graph model needs a GPU with compute support.
  On a device without it, the Editor reads **Grid rules and graph models cannot run on this device. They need a GPU with compute support.**

*[GPU]: Graphics processing unit
*[CPU]: Central processing unit
*[WGSL]: WebGPU Shading Language
*[TOML]: Tom's Obvious Minimal Language
