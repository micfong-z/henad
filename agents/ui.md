# UI text and styling

Rules for the app's text, corners and colours. Read before touching anything under
`crates/henad-app/src/ui`.

Everything under **Sentences** in `AGENTS.md` applies here too. On top of it:

- **Neutral tone.** No jargon unless the reader needs the precise word, and define an acronym the
  first time a panel uses one. A term the audience debugs with counts as necessary. A model
  author's kernel "panicked".
- **Conventional UI wording.** Use the words desktop and technical software already uses
  ("integer", "unavailable", "select", "default", "switch", "appear") over home-made phrasing
  ("whole number", "fill in", "stay at their values", "not available").
- **Sentence case.** "Model build failed", not "Model Build Failed".
- **Name a widget exactly as the widget is labelled.** The button reads Build, so prose says
  "press Build", and a field labelled Name is "the Name input field".
- **Tell the user what to do. A widget is never the subject.** "Press Show results to open them",
  not "Show results opens them". "Select another folder without results", not "Start needs a
  folder without results".
- **Tense follows meaning.** Present for a state ("Seed changed."), future for what will happen
  ("The model will be rebuilt.", "Results will appear as runs finish."), imperative for an
  instruction. Keep verbs simple, and avoid "should" and "would".
- **Short tooltips.** Say what the control does or what the user can do next. Leave out how it
  works inside and cross-references to the CLI unless the user needs them to act. "Seed for random
  number generation." is a finished tooltip.
- **No rounded corners.** `init.rs` sets every widget, window and menu radius to zero, and anything
  drawn outside those styles keeps to it: a `ProgressBar` takes `.corner_radius(0)` (egui draws a
  pill by default), and a chip, pill, badge or `Frame` gets `CornerRadius::ZERO`. A plot shows
  through `ui::show_plot`, since `egui_plot` paints its background with a fixed radius. egui gives
  the stripes of an `egui::Grid` a fixed radius of 2 as well. A key/value list goes through
  `ui::kv_grid`, whose rows end with `KvGridRows::end_row` and get square stripes. No other grid is
  striped. `egui_dock` draws the tab bar's scroll bar as a pill, and the dock hides it.
- **Colours come from the Micfong Colour System (MCS).** `ui/mcs.rs` holds one constant per entry
  (`mcs::BLUE_700`), and its module doc maps each state to entries: the state picks the hue, and
  the element carrying the colour picks the step. A badge, a fill or a coloured text takes its
  entry from that mapping in place of a `gamma_multiply` tint, a `Color32` literal or a theme
  colour borrowed as a fill. The theme's warn and error colours are orange 500 and red 500, and a
  status text reads them from the theme. The theme's neutral surfaces and strokes in `init.rs`
  predate MCS and stay as they are. Data colours (the plot series, the heatmap scale and its
  no-data grey) are not MCS and come from `ui/results/plot.rs`.
- **Chips of the sheet editor take MCS mappings** (`ui/editor/sheet/card_pane.rs`). A result chip
  fills with blue 900 and writes in blue 200, a result its line cannot use with red 900 and red
  200, and every other chip with gray 800 and gray 100, its lead word ("my") and the parameter's
  slider mark in gray 400. A blank is a gray 500 dashed outline around its word in gray 400, a
  select a gray 700 outline. The law box fills with gray 900, and a highlighted line with blue
  950. A state's swatch is the state's own colour, a data colour from the sheet. The primary
  action is blue 700, never the prototype's orange.
- **Swatches of the Sheet pane** (`ui/editor/sheet/widgets.rs`). A swatch is a square of the state's
  own colour with a gray 600 outline, gray 100 while hovered or focused. "Choose a colour" offers
  the twelve `henad_graph::sheet::edit::PALETTE_COLORS`, named by `COLOR_NAMES` (Black, Dark grey,
  Grey, White, Red, Orange, Yellow, Brown, Green, Dark green, Blue and Purple). They are data
  colours a state is drawn in, taken from the final editor's MCS entries, so they live in the sheet
  and not in `ui/mcs.rs`. New states take them in the neutral order of `STATE_COLORS`. A colour
  that is not in the palette, as in a starter, is named by the nearest one. "+ Add property" is
  the primary action, blue 700, while the sheet holds no property. A refused number field shows
  its message in the theme's error colour under its row. A parameter's Unit is free text with
  "per second" to "per year" offered under the field, never a select (D89). A Number property's
  shading shows a swatch of its hue from MCS step 900 to step 200 (`ShadeHue::ends`, data colours
  of the sheet like the state palette). The sheet toolbar's Undo and Redo show their icon and word.
- **The words rule of sheet mode.** The Editor tab in sheet mode and the text a sheet puts in the
  world tabs (names, measures, buttons, the tick counter) never show a word of
  `henad_graph::sheet::sentence::FORBIDDEN_WORDS` ("node", "seed", "compile" and the rest). The
  Model, Performance and System tabs are outside it, and so are the world tabs' own controls
  (Seed, Build, Unlimited TPS) and the controls that open or create a node graph (New node graph).
  Every word of sheet mode comes from `sheet::sentence`, and every accessible name there holds the
  control's visible words.
- **No superfluous politeness.** No "please", no apology.
- **"can" for ability, "might" for possibility.** Never "may".
- **Articles are optional.** "Load model" is as good as "Load the model". Drop the article when it
  buys nothing. In a button, a short label or a tooltip that is most of the time.

```text
The GPU reported an error while building the model.   // no, the article buys nothing
GPU reported an error while building the model.       // yes

Copy the message to the clipboard                     // no, tooltip
Copy message to clipboard                             // yes

Could not build the model                             // no, past modal
Model build failed                                    // yes

Seed changed but not applied. Press Build to apply.   // no, wordy
Seed changed. Press Build to apply.                   // yes

Results fill in as runs finish. Show results opens them.            // no, widget as subject
Results will appear as runs finish. Press Show results to open them. // yes

Varies one parameter at a time. The others stay at their Parameters tab values.    // no
Varies one parameter at a time. Other parameters use values from the Parameters tab. // yes
```
