# henad-app: dragging a card's lines

`ui/editor/sheet/card_drag.rs` moves a line of a card by drag and drop. The Card pane draws the
rows (`agents/henad-app-card.md`), and this module reads them. The line menu's Move up and Move
down stay the keyboard path, unchanged.

**The egui API.** egui's own drag and drop, at its lowest level: a handle that senses `Sense::DRAG`
alone (no `FOCUSABLE`, so it is no Tab stop and has no AccessKit node), `dnd_set_drag_payload` of a
`LineDrag` (card and line ids), `DragAndDrop::payload`, and egui's plugin for Escape and for the end
of the drag on release. `dnd_drag_source` paints the row's own widgets on a tooltip layer and
`dnd_drop_zone` restyles a frame, so neither gives an insertion line. egui_dnd was set aside: 0.17
is the release on egui 0.36 (0.18 needs 0.37), it reorders one flat list with animated reflow, and
a card is an outline whose ends of blocks need their own places. The tree views egui_ltreeview (0.9
on egui 0.36, 0.10 on 0.37) and egui_ailanthus (0.3 on 0.37) draw their own rows, selection and
keys, and a card's rows hold the sentence's own controls.

**The handle.** `kind_bar_ui` makes the row's gutter (the kind bar and the number column, up to
the bracket) the handle, under the id `CardState::widgets` with the line id and "drag", stable as
the pane scrolls. `handle_ui` paints `MDI_DRAG_VERTICAL` at `GRIP_SIZE` 14 left of the number
(`GRIP_OFFSET` 5 from the bar) while the pointer is over the row and no line is dragged, weak, and
in the text colour over the gutter, which shows the Grab cursor. The gutter's tooltip is
`sentence::drag_line_tooltip` of the kind's group word ("Change. Drag to move the line.").

**Slots.** A slot (`DropSlot`) is the block that would hold the line (`None` for the card's top
level) and the line it would go before (`None` for the end of that list). `card_ui` collects a
`DrawnRow` per drawn row in draw order, `RowOf::Line` and `RowOf::BlockEnd` for a block's add row
(`add_inside_ui` returns its rect), each with its rect and its body's left edge. Each gap between
drawn rows is one slot: the row under the gap names it (before a line, in that line's parent; above
a block's add row, at the end of that block), and the gap under the last row is the card's end. The
add row keeps the end of a block apart from the place after the block. The pointer picks the first
row whose centre lies below it. The gaps from the dragged row down to the end of its block are the
line's own place and show nothing.

**What may go where.** As a drag starts (`DragFrame::new`, past egui's click distance through
`is_decidedly_dragging`), `DragPlan::new` runs `SheetEdit::MoveLine` on a copy of the sheet up
until it is refused and then down, keeping each slot passed with the direction and the number of
moves (`Reach`). The plan lives in egui temp data under the pane's id with "henad_line_drag_plan"
until the drag ends, with the pointer's offset in the row where the drag started (`grab`). A slot
the moves reach can take the line, except a slot inside a Repeat or a block inside one for a line
`card_pane::looks_in_place` holds (Count, Look around, Pick a cell), refused with `LOOK_IN_REPEAT`
as the Repeat's add menu refuses them. A When or Repeat passes no slot inside another block, so
those read `DROP_INTO_BLOCK`. Over another card the copy reads `DROP_ON_OTHER_CARD`.

**A drop.** On release over a slot that can take the line, `DragFrame::place_ui` returns a
`DropMove`, and `card_ui` pushes a `CardEdit` that runs it with `After::Stay`: the plan's moves in
one edit, one step of undo, and then `slot_of` must name the slot, or the edit is refused in the
words of `DROP_MISSED` and changes nothing (a sheet that changed during the drag). Line ids,
references and keys stay as `MoveLine` leaves them, and the numbers follow the new display order.

**Looks.** While a line is dragged, its rows (the line, the lines inside it and its blocks' add
rows) draw at the theme's disabled opacity (`faded`, egui's `disabled_alpha`, through the painter's
opacity, so no id changes). `InsertionMark` paints a 2 point line (`INSERTION_WIDTH`) at the gap
from the body's left edge of the slot's level to the row's right edge, with a 6 point square at its
left end, blue 500 where the line can go and red 500 where it cannot, on a layer of its own that
`set_sublayer` keeps just above the copy, clipped to the pane. The copy (`ghost_ui`) is an `Area`
of `Order::Tooltip` in egui's popup frame with its shadow, at most `GHOST_WIDTH` 420 wide: per line
a kind bar, the number as a `small_label` in a column of 18 and the sentence in the body's font,
the lines inside a block indented 16 each, at most `GHOST_LINES` 4 and then `more_lines` weak, and
the refusal in the theme's error colour. It hangs `GHOST_GAP` 10 under the pointer, or under the
mark where the mark is lower, its left edge as far left of the pointer as the grab, so it hides
neither. A refusal sets the NotAllowed cursor. `DragFrame::finish` scrolls the pane by up to
`SCROLL_SPEED` 14 points a frame while the pointer is within `SCROLL_EDGE` 36 of the pane's top or
bottom and inside its width, and requests a repaint.

`src/tests/sheet_drag.rs` holds
`a_dropped_line_moves_as_the_line_menu_moves_it_in_one_step_of_undo` (Game of Life's line 3 to the
top equals two Move up, one Undo and Redo),
`a_line_drops_into_a_when_to_its_end_and_out_after_it` (the ants' line 16 between lines 13 and 14,
then above "+ Add inside line 12", then under it, back to the starter, each drop one step),
`a_when_moves_with_its_lines_but_not_into_another_when` (the copy shows `DROP_INTO_BLOCK`, nothing
changes, then line 9 to the top equals eight Move up),
`a_when_dropped_over_its_own_lines_stays_and_says_nothing`,
`a_line_does_not_go_to_another_card_and_a_look_does_not_go_inside_a_repeat`,
`escape_and_a_click_on_the_gutter_change_nothing`, `the_gutter_says_how_to_move_its_line`,
`the_card_pane_scrolls_while_a_line_is_dragged_at_its_edge` (1600 by 1000) and
`every_slot_a_drop_offers_is_reached_by_the_moves_of_the_line_menu` (every line of every starter
and of the ants with a When inside a When, to every slot: a line that is no block reaches each, a
block only those among its siblings and the blocks around it, through `DropMove::planned`).
