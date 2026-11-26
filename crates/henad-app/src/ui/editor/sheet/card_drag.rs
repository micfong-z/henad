//! Drag and drop of a card's lines: the handle in each row's gutter, the place a dragged line would go, the line that
//! marks that place, the copy of the line under the pointer, and the moves a drop runs.
//!
//! A *slot* is a place between two rows of a card where a line can go: the When or Repeat that would hold the line,
//! `None` for the card's top level, and the line it would go before, `None` for the end of that block or card. Each gap
//! between two drawn rows is one slot. The row that ends a block ("+ Add inside line 4") keeps the end of the block
//! apart from the place after it.
//!
//! A drop runs [`SheetEdit::MoveLine`] as many times as the line's menu would, in one edit and one step of undo. The
//! slots a line can reach are found by running those moves on a copy of the sheet as the drag starts. A When or Repeat
//! moves with its lines and never into another When or Repeat, and no line moves to another card. A Count, Look around
//! or Pick a cell does not go inside a Repeat, as the add menu of a Repeat offers none.

use egui::emath::Align2;
use egui::style::ScrollAnimation;
use egui::{
    Align, Area, CornerRadius, CursorIcon, DragAndDrop, FontId, Frame, Id, Label, LayerId, Layout, Order, Pos2, Rect,
    Response, RichText, Sense, Ui, Vec2, pos2, vec2,
};
use henad_graph::sheet::document::{LineKind, SheetCard, SheetDocument};
use henad_graph::sheet::edit::{MoveDirection, SheetBuilder, SheetEdit, SheetEditError};
use henad_graph::sheet::refusals;
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{AddButton, DROP_INTO_BLOCK, DROP_MISSED, DROP_ON_OTHER_CARD, more_lines};

use super::card_lines::CardLine;
use super::widgets::{KIND_BAR_WIDTH, kind_bar, small_label};
use crate::icons::material_design_icons::MDI_DRAG_VERTICAL;
use crate::ui::mcs;

/// Size in points of the grip that shows in the gutter of a row under the pointer.
const GRIP_SIZE: f32 = 14.0;

/// Thickness in points of the line that marks where a dragged line would go.
const INSERTION_WIDTH: f32 = 2.0;

/// Side in points of the square at the left end of the line that marks where a dragged line would go.
const INSERTION_END: f32 = 6.0;

/// Width in points of the copy of a dragged line at most.
const GHOST_WIDTH: f32 = 420.0;

/// Distance in points from the pointer, or from the line that marks the place under it where that line is lower, down
/// to the top of the copy of a dragged line.
const GHOST_GAP: f32 = 10.0;

/// Number of lines of a dragged block that its copy shows at most. A count stands for the rest.
const GHOST_LINES: usize = 4;

/// Width in points of the column of line numbers in the copy of a dragged line.
const GHOST_NUMBER_WIDTH: f32 = 18.0;

/// Indent in points of a line inside a dragged block in its copy.
const GHOST_INDENT: f32 = 16.0;

/// Height in points of the band at the top and the bottom of the Card pane where a drag scrolls the pane.
const SCROLL_EDGE: f32 = 36.0;

/// Distance in points the Card pane scrolls in one frame with the pointer at its edge.
const SCROLL_SPEED: f32 = 14.0;

/// The line a drag carries: the egui drag and drop payload of a row's handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineDrag {
    /// Id of the card that holds the line.
    pub card: String,
    /// Id of the line.
    pub line: String,
}

/// A place between two rows of a card where a line can go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropSlot {
    /// Id of the When or Repeat that would hold the line, `None` for the card's top level.
    pub parent: Option<String>,
    /// Id of the line the line would go before, `None` for the end of the block or the card.
    pub before: Option<String>,
}

/// A row drawn on a card: a line, or the row that ends a block.
#[derive(Debug, Clone)]
pub struct DrawnRow {
    /// Rect of the row.
    pub rect: Rect,
    /// What the row shows.
    pub row: RowOf,
    /// Left edge in points of the row's body, after the gutter and the indent of its blocks.
    pub body_left: f32,
}

/// What a drawn row shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowOf {
    /// The line of this id.
    Line(String),
    /// The row that ends the When or Repeat of this id, "+ Add inside line 4".
    BlockEnd(String),
}

/// A slot a dragged line can reach, with the moves of the line's menu that take it there.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Reach {
    /// The slot.
    slot: DropSlot,
    /// Direction of each move.
    direction: MoveDirection,
    /// Number of moves.
    steps: usize,
}

/// The slots a dragged line can reach, found as the drag starts, and where the line was grabbed. It lives in egui temp
/// data until the drag ends.
#[derive(Debug, Clone)]
struct DragPlan {
    /// The line.
    payload: LineDrag,
    /// Each slot the line's moves reach, nearest first in each direction.
    reaches: Vec<Reach>,
    /// Offset of the pointer from the top left corner of the line's row when the drag started, `None` until the row
    /// is drawn while dragged.
    grab: Option<Vec2>,
}

impl DragPlan {
    /// Returns the plan of the drag of `payload` over `document`: each slot that [`SheetEdit::MoveLine`] reaches, run
    /// on a copy of the sheet up until it is refused and then down until it is refused.
    fn new(document: &SheetDocument, payload: LineDrag) -> Self {
        let lines = document
            .cards
            .iter()
            .find(|card| card.id == payload.card)
            .map_or(0, |card| card.line_tables.len());
        // A move takes the line past one row, and a card has a row for each line and each block's end.
        let bound = 2 * lines + 2;
        let mut reaches = Vec::new();
        for direction in [MoveDirection::Up, MoveDirection::Down] {
            let mut builder = SheetBuilder::new(document.clone()).with_key_seed(0);
            for steps in 1..=bound {
                let edit = SheetEdit::MoveLine {
                    line: payload.line.clone(),
                    direction,
                };
                if builder.apply(edit).is_err() {
                    break;
                }
                let Some(slot) = slot_of(builder.document(), &payload.line) else {
                    break;
                };
                reaches.push(Reach { slot, direction, steps });
            }
        }
        Self {
            payload,
            reaches,
            grab: None,
        }
    }

    /// Returns the moves that take the line to `slot`, `None` for a slot they do not reach.
    fn reach(&self, slot: &DropSlot) -> Option<&Reach> {
        self.reaches.iter().find(|reach| reach.slot == *slot)
    }
}

/// Returns the slot line `line_id` sits in on its card of `document`, `None` for a line no card lists.
pub fn slot_of(document: &SheetDocument, line_id: &str) -> Option<DropSlot> {
    let (index, _) = document.line(line_id)?;
    let card = document.cards.get(index)?;
    let parent = card
        .line_tables
        .iter()
        .find(|(_, line)| line.lines.iter().any(|inner| inner == line_id))
        .map(|(id, _)| id.clone());
    let list = list_of(card, parent.as_deref())?;
    let position = list.iter().position(|id| id == line_id)?;
    Some(DropSlot {
        parent,
        before: list.get(position + 1).cloned(),
    })
}

/// Returns the ids of the lines inside `parent` on `card`, or the card's top-level lines for `None`.
fn list_of<'a>(card: &'a SheetCard, parent: Option<&str>) -> Option<&'a [String]> {
    match parent {
        None => Some(&card.lines),
        Some(parent) => card.line_tables.get(parent).map(|line| line.lines.as_slice()),
    }
}

/// A drop that moves a line: the moves of its menu that take it to its slot.
#[derive(Debug, Clone)]
pub struct DropMove {
    /// Id of the line.
    line: String,
    /// The slot the line goes to.
    slot: DropSlot,
    /// Direction of each move.
    direction: MoveDirection,
    /// Number of moves.
    steps: usize,
}

impl DropMove {
    /// Returns each drop of line `line` of card `card` on `document` that a drag offers, the moves of the line's menu
    /// that take the line to a slot, as a drag plans them when it starts.
    #[cfg(test)]
    pub fn planned(document: &SheetDocument, card: &str, line: &str) -> Vec<Self> {
        let payload = LineDrag {
            card: card.to_owned(),
            line: line.to_owned(),
        };
        DragPlan::new(document, payload)
            .reaches
            .into_iter()
            .map(|reach| Self {
                line: line.to_owned(),
                slot: reach.slot,
                direction: reach.direction,
                steps: reach.steps,
            })
            .collect()
    }

    /// The slot the line goes to.
    #[cfg(test)]
    pub fn slot(&self) -> &DropSlot {
        &self.slot
    }

    /// Runs the moves on `builder`.
    ///
    /// # Errors
    ///
    /// Returns the refusal of a move, or [`SheetEditError::Invalid`] in the words of [`DROP_MISSED`] when the moves
    /// leave the line elsewhere than its slot, as on a sheet that changed during the drag.
    pub fn run(self, builder: &mut SheetBuilder) -> Result<(), SheetEditError> {
        for _ in 0..self.steps {
            builder.apply(SheetEdit::MoveLine {
                line: self.line.clone(),
                direction: self.direction,
            })?;
        }
        if slot_of(builder.document(), &self.line) == Some(self.slot) {
            Ok(())
        } else {
            Err(SheetEditError::Invalid(DROP_MISSED.to_owned()))
        }
    }
}

/// One line of the copy of a dragged line or block.
#[derive(Debug, Clone)]
struct GhostLine {
    /// Group of the add button of the line's kind, which colours its bar.
    group: AddButton,
    /// Number of the line on its card.
    number: usize,
    /// Sentence of the line as plain text.
    sentence: String,
    /// Number of blocks around the line inside the dragged block.
    depth: usize,
}

/// The copy of a dragged line that follows the pointer.
#[derive(Debug, Clone)]
struct Ghost {
    /// The dragged line, then the lines inside it.
    lines: Vec<GhostLine>,
    /// Offset of the pointer from the top left corner of the dragged row when the drag started. The copy keeps the
    /// pointer as far from its left edge.
    grab: Vec2,
    /// Width of the copy in points.
    width: f32,
    /// Size of the sentences' words, as the card draws them.
    font: FontId,
}

/// State of a drag of a line in one frame of the Card pane, gathered as each card is drawn.
#[derive(Debug)]
pub struct DragFrame {
    /// Id the plan of the drag is kept under in egui temp data.
    plan_id: Id,
    /// Plan of the line dragged past the click distance, `None` while no line is dragged.
    plan: Option<DragPlan>,
    /// Position of the pointer.
    pointer: Option<Pos2>,
    /// Position where the pointer was pressed to start the drag, `None` for none.
    press_origin: Option<Pos2>,
    /// Whether the pointer was released in this frame, which drops the line.
    released: bool,
    /// Copy of the dragged line, set by its card.
    ghost: Option<Ghost>,
    /// Reason the line cannot go where the pointer is, `None` for a place it can go or no place.
    refusal: Option<&'static str>,
    /// Line that marks the place under the pointer, set by the card of the dragged line.
    mark: Option<InsertionMark>,
}

/// The line that marks where a dragged line would go, painted on a layer over the copy of the line.
#[derive(Debug, Clone, Copy)]
struct InsertionMark {
    /// Height of the line on the screen.
    y: f32,
    /// Left end of the line, the left edge of the body of a row at the place's level.
    left: f32,
    /// Right end of the line.
    right: f32,
    /// Whether the dragged line can go there.
    allowed: bool,
    /// Part of the screen the Card pane shows, which the line is clipped to.
    clip: Rect,
}

impl DragFrame {
    /// Returns the state of a drag of a line over `document` in this frame, the plan of the drag kept in egui temp data
    /// under `widgets`, the id the pane's controls are keyed under.
    pub fn new(ui: &Ui, document: &SheetDocument, widgets: Id) -> Self {
        let id = widgets.with("henad_line_drag_plan");
        let (dragging, pointer, press_origin, released) = ui.input(|input| {
            (
                input.pointer.is_decidedly_dragging(),
                input.pointer.interact_pos(),
                input.pointer.press_origin(),
                input.pointer.any_released(),
            )
        });
        let plan = DragAndDrop::payload::<LineDrag>(ui.ctx())
            .filter(|_| dragging)
            .map(|payload| {
                let kept = ui
                    .data(|data| data.get_temp::<DragPlan>(id))
                    .filter(|plan| plan.payload == *payload);
                kept.unwrap_or_else(|| {
                    let plan = DragPlan::new(document, (*payload).clone());
                    ui.data_mut(|data| data.insert_temp(id, plan.clone()));
                    plan
                })
            });
        if plan.is_none() {
            ui.data_mut(|data| data.remove::<DragPlan>(id));
        }
        Self {
            plan_id: id,
            plan,
            pointer,
            press_origin,
            released,
            ghost: None,
            refusal: None,
            mark: None,
        }
    }

    /// Returns the id of the line dragged from the card `card_id`, `None` while no line of that card is dragged.
    pub fn dragged_from(&self, card_id: &str) -> Option<&str> {
        self.plan
            .as_ref()
            .filter(|plan| plan.payload.card == card_id)
            .map(|plan| plan.payload.line.as_str())
    }

    /// Returns whether the row of line `line_id` moves with the dragged line of `relations`'s card: the line itself and
    /// the lines inside it.
    pub fn moves(&self, relations: &CardRelations<'_>, line_id: &str) -> bool {
        self.dragged_from(&relations.card().id)
            .is_some_and(|dragged| line_id == dragged || relations.is_within(line_id, dragged))
    }

    /// Marks the place under the pointer on the card `drawn` describes, and returns the drop of the dragged line on it
    /// when the pointer is released there.
    ///
    /// On the card of the dragged line the place is the gap between rows nearest the pointer: a blue line marks a place
    /// the line can go, and a red line one it cannot, with the reason in the copy under the pointer. The gaps around the
    /// dragged rows leave the line where it is. Over another card the copy says the line cannot go there.
    pub fn place_ui(&mut self, ui: &Ui, drawn: &DrawnCard<'_>) -> Option<DropMove> {
        let relations = drawn.relations;
        let inside = self.pointer.filter(|pointer| drawn.rect.contains(*pointer));
        let payload = self.plan.as_ref()?.payload.clone();
        if payload.card != relations.card().id {
            if inside.is_some() {
                self.refusal = Some(DROP_ON_OTHER_CARD);
            }
            return None;
        }
        let grab = self.grab(ui, drawn, &payload.line);
        self.ghost = line_ghost(ui, drawn, &payload.line, grab);
        let plan = self.plan.as_ref()?;
        let pointer = inside?;
        let rows = drawn.rows;
        let gap = rows
            .iter()
            .position(|row| pointer.y < row.rect.center().y)
            .unwrap_or(rows.len());
        let source = rows
            .iter()
            .position(|row| row.row == RowOf::Line(payload.line.clone()))?;
        let source_end = rows
            .iter()
            .position(|row| row.row == RowOf::BlockEnd(payload.line.clone()))
            .unwrap_or(source);
        if (source..=source_end + 1).contains(&gap) {
            return None;
        }
        let slot = match rows.get(gap).map(|row| &row.row) {
            Some(RowOf::Line(id)) => DropSlot {
                parent: relations.parent(id).map(str::to_owned),
                before: Some(id.clone()),
            },
            Some(RowOf::BlockEnd(block)) => DropSlot {
                parent: Some(block.clone()),
                before: None,
            },
            None => DropSlot {
                parent: None,
                before: None,
            },
        };
        let line = relations.line(&payload.line);
        let kind = line.map(|line| line.kind);
        let in_repeat = slot.parent.as_deref().is_some_and(|parent| {
            relations
                .line(parent)
                .is_some_and(|block| block.kind == LineKind::Repeat)
                || relations.repeat_of(parent).is_some()
        });
        let verdict = match plan.reach(&slot) {
            None if kind.is_some_and(LineKind::is_block) => Err(DROP_INTO_BLOCK),
            None => return None,
            Some(reach) => match line.filter(|_| in_repeat).and_then(refusals::in_repeat) {
                Some(refusal) => Err(refusal),
                None => Ok(reach),
            },
        };
        let (y, left) = match rows.get(gap) {
            Some(row) => (row.rect.top(), row.body_left),
            None => (
                rows.last().map_or(drawn.rect.bottom(), |row| row.rect.bottom()),
                drawn.top_body_left,
            ),
        };
        let right = rows.first().map_or(drawn.rect.right(), |row| row.rect.right());
        self.mark = Some(InsertionMark {
            y,
            left,
            right: right - 4.0,
            allowed: verdict.is_ok(),
            clip: ui.clip_rect(),
        });
        match verdict {
            Err(refusal) => {
                self.refusal = Some(refusal);
                None
            }
            Ok(reach) => self.released.then_some(DropMove {
                line: payload.line,
                slot,
                direction: reach.direction,
                steps: reach.steps,
            }),
        }
    }

    /// Returns the offset of the pointer from the top left corner of the row of `line_id` on `drawn` where the drag
    /// started, kept in the plan from the first frame the row is drawn while dragged.
    fn grab(&mut self, ui: &Ui, drawn: &DrawnCard<'_>, line_id: &str) -> Vec2 {
        if let Some(grab) = self.plan.as_ref().and_then(|plan| plan.grab) {
            return grab;
        }
        let row = drawn
            .rows
            .iter()
            .find(|row| row.row == RowOf::Line(line_id.to_owned()))
            .map(|row| row.rect.min);
        let grab = match (self.press_origin, row) {
            (Some(origin), Some(row)) => (origin - row).max(Vec2::ZERO),
            _ => Vec2::ZERO,
        };
        if let Some(plan) = &mut self.plan {
            plan.grab = Some(grab);
            ui.data_mut(|data| data.insert_temp(self.plan_id, plan.clone()));
        }
        grab
    }

    /// Draws the copy of the dragged line under the pointer with the reason it cannot go where the pointer is, sets the
    /// pointer's icon, and scrolls the Card pane while the pointer is near its top or bottom edge. `ui` is the pane's
    /// scrolled contents.
    pub fn finish(&self, ui: &Ui) {
        let (Some(_), Some(pointer)) = (&self.plan, self.pointer) else {
            return;
        };
        if self.refusal.is_some() {
            ui.ctx().set_cursor_icon(CursorIcon::NotAllowed);
        }
        // The copy hangs under the pointer and under the mark, so it hides neither.
        let top = self.mark.map_or(pointer.y, |mark| mark.y.max(pointer.y)) + GHOST_GAP;
        let ghost = self
            .ghost
            .as_ref()
            .map(|ghost| ghost_ui(ui, ghost, pos2(pointer.x - ghost.grab.x, top), self.refusal));
        if let Some(mark) = self.mark {
            mark.paint(ui.ctx(), ghost);
        }
        let visible = ui.clip_rect();
        if !visible.x_range().contains(pointer.x) {
            return;
        }
        let above = pointer.y - visible.top();
        let below = visible.bottom() - pointer.y;
        let speed = if above < SCROLL_EDGE {
            (SCROLL_EDGE - above.max(0.0)) / SCROLL_EDGE * SCROLL_SPEED
        } else if below < SCROLL_EDGE {
            -(SCROLL_EDGE - below.max(0.0)) / SCROLL_EDGE * SCROLL_SPEED
        } else {
            0.0
        };
        if speed != 0.0 {
            ui.scroll_with_delta_animation(vec2(0.0, speed), ScrollAnimation::none());
            ui.ctx().request_repaint();
        }
    }
}

/// A card as drawn, which a drag of a line reads.
#[derive(Debug)]
pub struct DrawnCard<'a> {
    /// Relations between the card's lines.
    pub relations: &'a CardRelations<'a>,
    /// The card's lines in display order.
    pub lines: &'a [CardLine],
    /// The card's rows in draw order.
    pub rows: &'a [DrawnRow],
    /// Rect of the whole card, its title and add buttons included.
    pub rect: Rect,
    /// Left edge in points of the body of a row at the card's top level.
    pub top_body_left: f32,
}

/// Returns the copy of line `line_id` of `drawn` and the lines inside it, `grab` the pointer's offset in its row.
fn line_ghost(ui: &Ui, drawn: &DrawnCard<'_>, line_id: &str, grab: Vec2) -> Option<Ghost> {
    let relations = drawn.relations;
    let top = drawn.lines.iter().find(|line| line.id == line_id)?;
    let lines = drawn
        .lines
        .iter()
        .filter(|line| line.id == line_id || relations.is_within(&line.id, line_id))
        .filter_map(|line| {
            Some(GhostLine {
                group: AddButton::for_kind(relations.line(&line.id)?.kind),
                number: line.number,
                sentence: line.sentence.clone(),
                depth: line.depth.saturating_sub(top.depth),
            })
        })
        .collect();
    let width = drawn.rows.first().map_or(GHOST_WIDTH, |row| row.rect.width());
    Some(Ghost {
        lines,
        grab,
        width: width.min(GHOST_WIDTH),
        font: egui::TextStyle::Body.resolve(ui.style()),
    })
}

impl InsertionMark {
    /// Paints the line across a card from its left end to its right end, with a square at its left end, in blue 500
    /// where the dragged line can go and red 500 where it cannot. The line lies on a layer of its own just above the
    /// layer `ghost` of the copy of the dragged line, where there is one.
    fn paint(self, ctx: &egui::Context, ghost: Option<LayerId>) {
        let layer = LayerId::new(Order::Tooltip, Id::new("henad_line_drag_mark"));
        if let Some(ghost) = ghost {
            ctx.set_sublayer(ghost, layer);
        }
        let painter = ctx.layer_painter(layer).with_clip_rect(self.clip);
        let color = if self.allowed { mcs::BLUE_500 } else { mcs::RED_500 };
        let line = Rect::from_min_max(
            pos2(self.left, self.y - INSERTION_WIDTH / 2.0),
            pos2(self.right, self.y + INSERTION_WIDTH / 2.0),
        );
        painter.rect_filled(line, CornerRadius::ZERO, color);
        let end = Rect::from_center_size(pos2(self.left, self.y), Vec2::splat(INSERTION_END));
        painter.rect_filled(end, CornerRadius::ZERO, color);
    }
}

/// Draws `ghost` with its top left corner at `corner`, in egui's popup frame, its words wrapping at its width, and
/// `refusal` under its lines in the theme's error colour. Returns the layer it is drawn on.
fn ghost_ui(ui: &Ui, ghost: &Ghost, corner: Pos2, refusal: Option<&str>) -> LayerId {
    let number_height = ui.fonts_mut(|fonts| fonts.row_height(&ghost.font));
    let area = Area::new(Id::new("henad_line_drag_ghost"))
        .order(Order::Tooltip)
        .fixed_pos(corner)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(ghost.width);
                ui.spacing_mut().item_spacing = vec2(0.0, 4.0);
                for (index, line) in ghost.lines.iter().enumerate() {
                    if index == GHOST_LINES {
                        let more = more_lines(ghost.lines.len() - GHOST_LINES);
                        ui.label(RichText::new(more).font(ghost.font.clone()).weak());
                        break;
                    }
                    let row = ui.horizontal_top(|ui| {
                        ui.add_space(KIND_BAR_WIDTH + GHOST_INDENT * line.depth as f32);
                        ui.allocate_ui_with_layout(
                            vec2(GHOST_NUMBER_WIDTH, number_height),
                            Layout::right_to_left(Align::Center),
                            |ui| ui.add(Label::new(small_label(line.number.to_string())).selectable(false)),
                        );
                        ui.add_space(8.0);
                        ui.vertical(|ui| {
                            let words = RichText::new(line.sentence.as_str()).font(ghost.font.clone());
                            ui.add(Label::new(words).wrap().selectable(false));
                        });
                    });
                    kind_bar(ui.painter(), row.response.rect, line.group);
                }
                if let Some(refusal) = refusal {
                    ui.add_space(2.0);
                    let words = RichText::new(refusal).color(ui.visuals().error_fg_color);
                    ui.add(Label::new(words).wrap().selectable(false));
                }
            });
        });
    area.response.layer_id
}

/// Draws the handle of a line's row: the gutter `gutter`, dragged to move the line `payload` names under the id `id`,
/// and returns its response. While the pointer is over the row `row` and no line is dragged, a grip shows centred at
/// `grip`, weak, and in the text colour under the pointer.
///
/// The handle takes no keyboard focus. The line's menu moves the line from the keyboard.
pub fn handle_ui(ui: &Ui, gutter: Rect, row: Rect, id: Id, payload: LineDrag, grip: Pos2) -> Response {
    let response = ui.interact(gutter, id, Sense::DRAG);
    response.dnd_set_drag_payload(payload);
    if !DragAndDrop::has_any_payload(ui.ctx()) && ui.rect_contains_pointer(row) {
        let color = if response.hovered() {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        ui.painter().text(
            grip,
            Align2::CENTER_CENTER,
            MDI_DRAG_VERTICAL,
            FontId::proportional(GRIP_SIZE),
            color,
        );
    }
    response
}

/// Runs `add` on `ui` at the theme's opacity of a disabled widget when `faded` holds, as the rows of a dragged line
/// show while it is dragged.
pub fn faded<R>(ui: &mut Ui, faded: bool, add: impl FnOnce(&mut Ui) -> R) -> R {
    if !faded {
        return add(ui);
    }
    let opacity = ui.opacity();
    ui.multiply_opacity(ui.visuals().disabled_alpha());
    let result = add(ui);
    ui.set_opacity(opacity);
    result
}
