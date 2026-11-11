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
  colour borrowed as a fill. A node of a converted copy added since the conversion is green, and a
  node changed since then is yellow (`ChangeMark::colors`, badge 900 and 200, stroke 500). The
  theme's warn and error colours are orange 500 and red 500, and a status text reads them from the
  theme. The theme's neutral surfaces and strokes in `init.rs` predate MCS and stay as they are.
  Data colours (the plot series, the heatmap scale and its no-data grey) are not MCS and come from
  `ui/results/plot.rs`.
- **One palette rule: the meaning picks the hue, and the element picks the step.** No step outside
  this table without adding its row here.

  | Element | Step |
  | --- | --- |
  | Tinted fill with text on it (chip, badge, current item, open control) | hue 900, text hue 200 |
  | Faint row tint (the highlighted line only) | hue 950 |
  | Solid fill (primary action, progress) | hue 700, text gray 50 |
  | Line, bar, outline or icon in a hue (kind bar, focus outline, notice bar) | hue 500 |
  | Status text straight on a panel | orange 500, red 500 (the theme's warn and error) |
  | Neutral chip | gray 800 fill, gray 100 text, gray 400 lead word or glyph |
  | Neutral outline of a control | gray 700 |
  | Dashed outline of an empty slot (blank, empty select, add button) | gray 500, word gray 400 |
  | Hairline (separator, guide, a region's border) | gray 800 |
  | Region ground (a card, a section of the Sheet pane) | gray 900, the window fill |
  | Overlay border (picker, menu, select list) | gray 600 |

- **Style: flat, framed regions, hairlines inside.** No shadow, gradient or animation. A card is a
  region of gray 900 inside a 1 point gray 800 border, 12 points from the next, with its title row
  at the top (the heading in the semibold face, the count as a small upper-case mono label) and
  its add buttons in a footer under a hairline. Rows are split by gray 800 hairlines, and a block
  is its indent and a 1 point gray 800 guide per level, with no tinted band. Metadata (line
  numbers, fire counts, counts, titles of overlays and their group labels) is mono 11 in gray 400,
  upper case for a title or a count. Sentences are 13 points, and the semibold face is for lead
  words and the card heading alone.
- **The Card pane's components** (`ui/editor/sheet/widgets.rs`, one function each, no call site
  passes a colour). Keyboard focus is a 1.5 point blue 500 outline on every custom control, and
  the row that holds it marks its gutter with a 2 point blue 500 line under the line number.
  - Chip, 20 high: a result blue 900 under blue 200, a result its line cannot use red 900 under
    red 200 and struck through, every other chip gray 800 under gray 100 with a gray 400 lead word
    or slider mark, a number in the mono face, "any state" in italics. Hover outlines it in its
    text colour, and an open picker turns any chip blue 900 inside a blue 500 outline.
  - Blank: gray 500 dashes around its word in gray 400, solid on hover with the word gray 200,
    blue 900 while open. The optional "for each ..." is quiet (gray 700 dashes, weak word) until
    its line is hovered or focused.
  - Inline select: gray 700 outline, gray 400 on hover, blue 500 while open, dashed while empty.
    Condition arrow: a 14 point chevron box in gray 700, gray 400 on hover, blue 500 while open,
    its hit area the row's height. Text control ("always", "for each ...", "+"): weak text, a gray
    700 outline on hover or while open.
  - Line menu button: weak dots in a fixed column, the theme's hover fill, blue 900 while open.
    Add button: dashed gray 500 with the kind's 6 point square, solid on hover, blue 500 while
    open. A note's button: gray 700 outline, gray 400 on hover.
  - Overlay (picker, line menu, add menus, select list): the theme's window fill inside gray 600,
    opaque from its first frame, a small upper-case title and group labels with a hairline. An
    item is a full-width row, gray 800 under the pointer or the keyboard focus, blue 900 with a
    check and blue 200 words when current, weak with its reason as a tooltip when it cannot apply.
    A state shows its swatch, a square of its own colour inside gray 700.
  - Notice: a 2 point blue 500 bar and a blue 500 icon before the text, one label. Segmented
    toggle (Lines, Diagram): weak while off, the text colour over a 2 point blue 500 line while on.
  The theme in `init.rs` is unchanged by the Card pane. The primary action is blue 700, never the
  prototype's orange.
- **The Sheet pane's sections** (`ui/editor/sheet/sheet_pane/layout.rs` over the components of
  `widgets.rs`). Each section is a region (`widgets::region`, as a card is), `REGION_GAP` 12 from
  the next, its title a small upper-case mono label with a hairline to its "?" at the right edge
  (`widgets::region_title`), its "+ Add ..." button the last row inside it, dashed
  (`widgets::add_button` with no kind). Form rows put every label of a section in one column and
  its fields in the next, number fields right-aligned (`commit_number`), a hint weak and a refused
  field's message red under its field. Tables (properties, parameters, plotted measures, buttons,
  events) have a header row of small upper-case mono labels, a gray 800 hairline under it and under
  each row, the theme's hover fill under the row under the pointer, and the delete button, a weak
  "×" text control, in a right-aligned last column. A table's columns give up width in an order
  its function's doc states, a long select or chip ends in an ellipsis with its words as the
  tooltip, and a table that still does not fit scrolls sideways on its own. Every select of the
  pane is the inline select with its list in an overlay (`select_within`, `select_list`), and the
  pickers ("Choose a parameter", a chip's picker, "Choose a colour") are overlays too.
- **A hue names a line's kind in one place.** The bar at the left of a card's row, and the square
  on its add button, take the kind's hue at step 500: Look around blue, Chance purple, Yes or no
  yellow, Change green, Calculate gray, When and Repeat gray 100 (`card_pane::kind_hue`, over
  `AddButton::for_kind`). The Diagram's boxes take the same bar. Everywhere else a hue still means
  a state: blue the primary action and a result, orange a warning, red a failure.
- **Swatches of the Sheet pane** (`ui/editor/sheet/widgets.rs`). A swatch button is a 14 point
  square of the state's own colour inside gray 700, gray 100 while hovered, blue 500 while its
  picker is open, and the focus outline while focused. A swatch beside a state's name
  (`swatch_ui`) is the chip's 9 point square inside gray 700. "Choose a colour" offers
  the twelve `henad_graph::sheet::edit::PALETTE_COLORS`, named by `COLOR_NAMES` (Black, Dark grey,
  Grey, White, Red, Orange, Yellow, Brown, Green, Dark green, Blue and Purple). They are data
  colours a state is drawn in, taken from MCS entries, so they live in the sheet
  and not in `ui/mcs.rs`. New states take them in the neutral order of `STATE_COLORS`. A colour
  that is not in the palette, as in a starter, is named by the nearest one. "+ Add property" is
  the primary action, blue 700, while the sheet holds no property. A refused number field shows
  its message in the theme's error colour under its row. A parameter's Unit is free text with
  "per second" to "per year" offered under the field, never a select. A Number property's
  shading shows a swatch of its hue from MCS step 900 to step 200 (`ShadeHue::ends`, data colours
  of the sheet like the state palette). The sheet toolbar's Undo and Redo show their icon and word.
- **The words rule of sheet mode.** The Editor tab in sheet mode and the text a sheet puts in the
  world tabs (names, measures, buttons, the tick counter) never show a word of
  `henad_graph::sheet::sentence::FORBIDDEN_WORDS` ("node", "seed", "compile" and the rest), nor one
with a plural or verb ending (`is_forbidden_word`). The
  Model, Performance and System tabs are outside it, and so are the world tabs' own controls
  (Seed, Build, Unlimited TPS) and the controls that open or create a node graph: New node
  graph, Convert to node graph, Edit as node graph, the conversion dialog and the sheet's line
  "Converted to node graph: X". The tests' `opens_a_node_graph` matches those controls by their
  whole names, and a test leaves the dialog out by place with `Harness::accessible_texts_outside`.
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
