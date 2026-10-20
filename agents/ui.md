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
