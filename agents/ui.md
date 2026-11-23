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
  theme's neutral surfaces and strokes in `init.rs` predate MCS and stay as they are. Data colours
  (the plot series, the heatmap scale and its no-data grey) are not MCS and come from
  `ui/results/plot.rs`.
- **egui's own look for overlays** (`init.rs`). Every popup, menu, tooltip, combo list and
  window keeps egui's frame from the theme: the window fill inside a 1 point gray 800
  `window_stroke`, with `popup_shadow` (black at alpha 96, blur 8, offset (6, 10)) under a popup
  or a menu and `window_shadow` under a window. The Editor's own overlays draw `Frame::popup` and
  its dialogs `Frame::window`, never a border of their own. The selection is blue 700 under 1
  point of (192, 222, 255), so a list's current item, selected text, a slider's trailing fill and
  the outline of a focused text field are egui's. The focus outline is egui's too,
  `widgets.active.bg_stroke`, 1 point of white: egui draws a focused button, checkbox, radio or
  combo box in it, a pressed button shows it too, and the Editor's own controls paint the same
  stroke through `widgets::focus_ring`.
- **Every stop of Tab has an accessible name.** A resizable `Panel` draws inside
  `focus::without_focus_stop` with `focus::panel_divider` of its id, which takes its divider out of
  the order, and Tab from the last widget moves on to the first (`focus::install`). The tab bars and
  splitters of `egui_dock` are outside this rule.
- **One palette rule: the meaning picks the hue, and the element picks the step.** No step outside
  this table without adding its row here.

  | Element | Step |
  | --- | --- |
  | Tinted fill with text on it (chip, badge) | hue 900, text hue 200 |
  | Faint row tint (the highlighted line only) | hue 950 |
  | Solid fill (primary action, progress) | hue 700, text gray 50 |
  | Line, bar, outline or icon in a hue (kind bar, a row's focus mark, notice icon) | hue 500 |
  | Status text straight on a panel | orange 500, red 500 (the theme's warn and error) |
  | Neutral chip | gray 800 fill, gray 100 text, gray 400 lead word or glyph |
  | Neutral outline (a swatch) | gray 700 |
  | Outline of an empty slot (blank, empty select or card) or add button | gray 800, 600 on hover |
  | Hairline (separator, guide, a region's border) | gray 800 |
  | Region ground (a card) | gray 900, the window fill |
  | Button, select, open control, current item | the theme's widget visuals and selection |

- **Style: flat, framed cards, hairlines inside.** No shadow of the Editor's own (egui's popups and
  windows keep theirs), gradient or animation. A card is a region of gray 900 inside a 1 point gray
  800 border, 12 points from the next, with its title row at the top (the heading as a `title_text`,
  the count as a `caption`) and its add buttons in a footer under a hairline. Rows are split by gray
  800 hairlines, and a block is its indent and a 1 point gray 800 guide per level, with no tinted
  band. No label is in upper case. A title (a card, a section of the Sheet pane, an overlay, a
  dialog) is `title_text`: the semibold face at the body size in gray 100, in sentence case.
  Metadata (line numbers, fire counts, counts, table headers and the group labels of an overlay) is
  `small_label` or `caption`: the proportional face at the theme's small size in gray 400. The mono
  face is for a value shown as typed or as code alone: a number in a field or a chip, a formula, and
  the WGSL and TOML views. Sentences are 13 points, and the semibold face is for lead words and
  titles.
- **The Card pane's components** (`ui/editor/sheet/widgets.rs`, one function each, no call site
  passes a colour). They look as egui's stock widgets do, as the buttons of the Playback, Parameters
  and Sweep tabs: `paint_button` paints a control the Editor allocates itself (to keep the widget id
  that opens or focuses it) in the theme's widget visuals of its state, the fill, outline,
  expansion, square corners and padding of egui's `Button`, and a control that needs no id of its
  own is an egui `Button`. Keyboard focus is the theme's active outline (1 point of white), and
  the row that holds it marks its gutter with a 2 point blue 500 line under the line number. A
  control whose popup is open takes the look of a selected button: the selection fill and text
  colour. The add buttons, blanks and empty selects follow the empty-slot rule below instead.
  - Chip, 20 high: a button in egui's frame that keeps its own fill, a result blue 900 under blue
    200, a result its line cannot use red 900 under red 200 and struck through, every other chip
    gray 800 under gray 100 with a gray 400 lead word or slider mark, a number in the mono face,
    "any state" in italics. Hover, focus and an open picker follow the button, but a red chip
    keeps its colours while its picker is open, inside the selection's outline.
  - Blank: its word weak inside a 1 point gray 800 outline (`slot_outline`), gray 600 on hover
    with the word in the text colour, selected while open. The optional "for each …" is its weak
    word alone, with no outline, until its line is hovered or focused. Nothing is dashed.
  - Inline select: egui's combo box look, the button frame with its words and chevron in the
    widget text colour and `widgets.open` while its list is open, and an empty slot's outline and
    weak words while empty. Condition arrow: a frameless button, a chevron with egui's button frame
    under the pointer, on focus and while open, its hit area the row's height. Text control
    ("always", "for each", "+", the "×" of a row): the same frameless button with weak words.
  - Line menu button: the same frameless button with weak dots, in a fixed column. Add button:
    weak words inside the empty slot's gray 800 outline, after the kind's 6 point square on a card,
    gray 600 with the words in the text colour on hover or while its menu is open. A note's button
    and the segmented toggle (Lines, Diagram) are egui's `Button`, small and selectable.
  - Overlay (picker, line menu, add menus, select list): egui's popup frame (`Frame::popup`),
    opaque from its first frame, a hairline under its title and above each group label
    (`overlay_group`), but for a group right under the title, which shares the title's
    (`a_group_right_under_an_overlay_title_takes_no_second_hairline`). An
    item is a full-width row in the theme's hover fill under the pointer or the keyboard focus,
    the selection fill with a check and its words in the selection's text colour when current,
    weak with its reason as a tooltip when it cannot apply.
    A state shows its swatch, a square of its own colour inside gray 700. Every overlay is `placed`:
    under its anchor, over it where it does not fit under it, and otherwise on the side with more
    room and as tall as that room. With less than `OVERLAY_MIN_ROOM` (240 points) on both sides it
    opens at the top of the window and takes the window's height. The fit uses the overlay's height
    in the frame before, and its room (`overlay_room`) stays in egui temp data under its id, inside
    `OVERLAY_SCREEN_GAP` of the window's edges. An add menu's items scroll under its title in a
    scroll area as tall as the room leaves (`overlay_items`). A picker that scrolls keeps a Number
    box that comes last, and its Remove item, under its scroll area. A picker of
    either pane is `widgets::PICKER_WIDTH` (300 points) wide at least, its words wrapping at that
    width, so pickers open as wide as each other. A select's list and a menu take their content's
    width. In the AccessKit tree a picker or an add menu is a group of role `Dialog` named by its
    title, and its title a heading (`overlay_title`). The items of a picker or a menu are
    `menu_item`s, role `MenuItem`, inside `menu_group`s, role `Menu`, each labelled through
    `labelled_by` by its group's label, by the overlay's title for a group with no label, or by the
    button that opens a menu with no title, the line menu. A select's options stay buttons.
    ArrowDown and ArrowUp move the keyboard focus through an overlay's enabled rows
    (`move_through_rows`, over the rows `overlay_item` recorded in the frame before): to the next or
    the one before, round at either end, and from outside the overlay into its first or its last
    row. A text field of the overlay that holds the focus keeps the keys, the Number box included
    (`TextEdit::load_state` tells a field from a button). A picker's box that holds no text, marked
    by `mark_empty_box`, passes them on. From such a box, or from a button of the overlay other than
    a row, such as Use beside the Number box, the keys move into the first or the last row, as from
    outside. A row that gains the focus outside its overlay's clip rect scrolls into view. A row
    in view does not call `scroll_to_me`, which would scroll the pane under an overlay that has no
    scroll area of its own. Escape closes an overlay and returns the focus to the control that
    opened it (`escape_to_anchor`, or `CardPicker::show` for a card's picker).
  - Tooltip of a focusable control in sheet mode, of a button of the Editor's header and of a sheet
    button in the world: `FocusTooltip::on_hover_or_focus_text`, which shows the tooltip under the
    pointer and while the keyboard holds the control's focus, never while a popup is open. The focus
    counts as the keyboard's while the last press was a key (`key_pressed_last`, kept in egui temp
    data), so a control focused after a click shows none. The focus tooltip senses hover alone and
    is not interactable, as egui's own tooltip of plain text, so a click under it reaches the
    control there. A label, a hover-only mark and an item inside an open overlay keep
    `on_hover_text`.
  - "?" help of a heading (`help::help_mark`): a label that takes no click and no keyboard focus.
    The weak `MDI_HELP_CIRCLE_OUTLINE` sits inside the theme's horizontal button padding and senses
    hover alone, with no frame or fill under the pointer and the default cursor. Its tooltip
    (`on_hover_ui`) shows under the pointer only.
  - Dialog (`dialog_frame`, `dialog_title`, `dialog_buttons`, the New model, New parameter, Rebuild
    model, conversion, unsaved changes, replace applied model and delete dialogs): egui's window
    frame (`Frame::window`) with 12 points of margin, its title (`title_text`) with a gray 800
    hairline under it across the dialog, and the buttons in one row at the bottom right, the primary
    last (blue 700, or red 900 under red 200 for Rebuild, Replace and Delete). Enter presses the
    primary unless another button took the key or the dialog opened on that frame, and Escape closes
    through `Modal`. `dialog_title` marks the dialog's AccessKit node modal and makes the title a
    heading (`heading_caption`, a `caption` of role `Heading` named by its words as written). A list
    of choices is `choice_row`s: full-width radio buttons with a weak description under the name,
    the theme's hover fill under the pointer or the focus, the selection fill with a check and its
    words in the selection's text colour when chosen, as an overlay's current item (`list_row_fill`
    and `paint_list_row_marks` paint both). A list that can outgrow the window scrolls in
    `dialog_list`, which keeps the buttons in view.
  - Notice (`Notice`, `notice_ui` for information): an icon in the hue of its kind at step 500
    before the text in the text colour, one label, with no bar at its left: blue and an
    information icon for information, orange and a triangle for a warning, red and an exclamation
    mark for an error. Small buttons follow the text, and weak notes sit under it inside the row.
    Every message of the Editor is one, in the status area under the panes or, for a converted
    copy's banner, under the header.
  The theme in `init.rs` is unchanged by the Card pane. The primary action is blue 700, never the
  prototype's orange.
- **The Sheet pane's sections** (`ui/editor/sheet/sheet_pane/layout.rs` over the components of
  `widgets.rs`). Each section stands on the pane's own ground with no frame, `REGION_GAP` 12 from
  the next, its title a `title_text` with a hairline to its "?" at the right edge
  (`widgets::region_title`), its "+ Add ..." button the last row inside it
  (`widgets::add_row_button` with no kind). Form rows put every label of a section in one column and
  its fields in the next, number fields right-aligned (`commit_number`), a hint weak and a refused
  field's message red under its field. Tables (properties, parameters, plotted measures, buttons,
  events) have a header row of `caption`s, a gray 800 hairline under it and under
  each row, the theme's hover fill under the row under the pointer, and the delete button, a weak
  "×" text control, in a right-aligned last column. A table's columns give up width in an order
  its function's doc states, a long select or chip ends in an ellipsis with its words as the
  tooltip, and a table that still does not fit scrolls sideways on its own. Every select of the
  pane is the inline select with its list in an overlay (`select_within`, `select_list`), and the
  pickers ("Choose a parameter", a chip's picker, "Choose a colour") are overlays too.
- **A hue names a line's kind in one place.** The bar at the left of a card's row, and the square
  on its add button, take the kind's hue at step 500: Look around blue, Chance purple, Yes or no
  yellow, Change green, Calculate gray, When and Repeat gray 100 (`widgets::kind_hue`, over
  `AddButton::for_kind`). The Diagram's boxes take the same bar. Everywhere else a hue still means
  a state: blue the primary action and a result, orange a warning, red a failure.
- **Swatches of the Sheet pane** (`ui/editor/sheet/widgets.rs`). A swatch button is a 14 point
  square of the state's own colour inside gray 700, the theme's hover outline while hovered, the
  selection's outline while its picker is open, and the focus outline while focused. A swatch beside a state's name
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
  of the sheet like the state palette). The header's Undo and Redo show their icon and word.
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
