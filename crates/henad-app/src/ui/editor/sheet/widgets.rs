//! Controls the Sheet pane and the Card pane share: the chip, the blank, a number field that commits on Enter
//! or on losing focus, the picker, the colour palette and the New parameter dialog. The Editor's notices and the parts
//! of its dialogs live here too.
//!
//! A *picker* is a popover under its anchor that lists what a blank or a field can take, in groups under headings, with
//! "New parameter…" and a box for a typed number. A group with no item is left out, and a picker with nothing to
//! offer says what is missing. A typed number takes a trailing "%" as a percent ("30%" is 0.3).

use egui::{
    Button, Color32, CornerRadius, FontId, Frame, Id, Key, Margin, Modal, Painter, Popup, PopupCloseBehavior, Pos2,
    Rect, RectAlign, Response, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, TextStyle, Ui, Vec2, WidgetInfo,
    WidgetType, pos2, vec2,
};
use henad_graph::sheet::check::ValueKind;
use henad_graph::sheet::document::{ParameterType, PropertyType, SheetDocument, SheetLiteral};
use henad_graph::sheet::edit::{IdSpace, PALETTE_COLORS, SheetEdit, SheetEditError, is_reserved_name};
use henad_graph::sheet::format_number;
use henad_graph::sheet::sentence::{
    ARROW_KIND, CELL_KIND, HEADING_KIND, INTEGER_KIND, NUMBER_KIND, POSITION_KIND, STATES_KIND, YES_NO_KIND,
    type_symbol_name,
};
use henad_graph::sheet::sentence::{
    AddButton, CANCEL, CHANCE_BOX_HINT, CHANCE_LIMITS_OUTSIDE, CHANCE_OUT_OF_RANGE, CHANCE_PARAMETER_PLACEHOLDER,
    CHOOSE_A_COLOR, COLOR_NAMES, CREATE_PARAMETER, ChipKind, DEFAULT_LABEL, DEFAULT_NAME, DEFAULT_OUTSIDE_LIMITS,
    DIALOG_NAME_MISSING, DIALOG_UNIT_PLACEHOLDER, KEPT_OUT_OF_RANGE, LIMITS_NOT_NUMBERS, MAX_LABEL, MIN_LABEL,
    MIN_NOT_BELOW_MAX, NAME_LABEL, NEW_PARAMETER_HEADING, NEW_PARAMETER_LEAD, NUMBER_BOX_HINT,
    NUMBER_PARAMETER_PLACEHOLDER, PARAMETER_NAME_FIELD, RATE_BELOW_ZERO, RATE_PARAMETER_PLACEHOLDER, SHARE_BOX_HINT,
    SHARE_LIMITS_OUTSIDE, SHARE_PARAMETER_PLACEHOLDER, SentenceChip, TIMES_LIMITS_NOT_INTEGERS,
    TIMES_PARAMETER_PLACEHOLDER, TYPE_ONE_NUMBER, TYPE_TIMES, UNIT_LABEL, UNSAVED_MARK, UNSAVED_MARK_TOOLTIP,
    parameter_name_taken, read_typed_number,
};
use henad_graph::value::Color;

use super::{accessible_bounds, accessible_group};
use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_ALERT_CIRCLE, MDI_ARROW_TOP_RIGHT_THIN, MDI_CHECK, MDI_CHEVRON_DOWN, MDI_CIRCLE_MEDIUM,
    MDI_CIRCLE_SMALL, MDI_COMPASS_OUTLINE, MDI_DOTS_HORIZONTAL, MDI_INFORMATION, MDI_LABEL_OUTLINE,
    MDI_MAP_MARKER_OUTLINE, MDI_POUND, MDI_SQUARE_OUTLINE, MDI_TOGGLE_SWITCH_OUTLINE, MDI_TUNE_VERTICAL,
};
use crate::ui::mcs;
use crate::ui::sweep::layout::{destructive_button, filled_button};
use crate::ui::text::capitalize;

/// Padding in points at each side of the words of a chip, a blank or a select, the theme's button padding.
pub const CHIP_PADDING: f32 = 4.0;

/// Height in points of a chip, a blank or a select.
pub const CHIP_HEIGHT: f32 = 20.0;

/// Gap in points between a chip's glyph, swatch or lead word and its name.
const CHIP_GAP: f32 = 4.0;

/// Side in points of the swatch of a state chip.
pub const CHIP_SWATCH: f32 = 9.0;

/// Padding in points inside an overlay, and at the sides of its items, the theme's menu margin.
pub const OVERLAY_PADDING: f32 = 6.0;

/// Height in points of an item of an overlay with no grey words under it.
const OVERLAY_ROW: f32 = 22.0;

/// Space in points an overlay leaves between its frame and an edge of the window.
pub const OVERLAY_SCREEN_GAP: f32 = 8.0;

/// Least height in points that a side of its anchor offers an overlay that fits on neither side. With less room on both
/// sides, the overlay takes the window's height from its top.
const OVERLAY_MIN_ROOM: f32 = 240.0;

/// Key in egui temp data, with an overlay's id, of the most height in points the overlay takes.
const OVERLAY_ROOM: &str = "henad_overlay_room";

/// Width in points of the column at the left of an overlay's items that holds the check of the current value.
const MARK_COLUMN: f32 = 16.0;

/// Size in points of the small icons of the editor: a select's chevron and the check of an overlay's current item.
pub const SMALL_ICON: f32 = 11.0;

/// Size in points of the grey words under an item of an overlay.
const OVERLAY_DETAIL: f32 = 11.5;

/// Key in egui temp data of the rows of the overlay being drawn, and with an overlay's id of the rows it drew last.
const OVERLAY_ROWS: &str = "henad_overlay_rows";

/// Salt of the id under which a box of an overlay keeps whether it holds no text, below the box's id.
const EMPTY_BOX: &str = "henad_overlay_empty_box";

/// Key in egui temp data of whether the last press was a key rather than a pointer button.
const KEY_PRESSED_LAST: &str = "henad_key_pressed_last";

/// Side in points of the square of a kind's hue on an add button.
const ADD_SQUARE: f32 = 6.0;

/// Side in points of the chevron box after a condition word.
const ARROW_SIDE: f32 = 14.0;

/// Width in points of the place of the chevron box after a condition word, its box with a gap at each side.
pub const ARROW_WIDTH: f32 = ARROW_SIDE + 4.0;

/// Width in points of the bar at the left edge of a card's row or a Diagram box, in the hue of its line's kind.
pub const KIND_BAR_WIDTH: f32 = 3.0;

// Custom components
//
// Each control the sheet editor paints itself is drawn by one function here, and no call site passes a colour. A button
// takes the theme's widget visuals of its state, as egui's own does ([`paint_button`]). The other colours follow the
// palette in `agents/ui.md`: a chip's fill is a hue's 900 with its 200 for text, a line or an outline in a hue is its
// 500, a neutral chip is gray 800 with gray 100 text, an empty slot a weak word inside a gray 800 outline, a hairline
// gray 800, and an overlay egui's popup frame.

/// Space in points between two cards, and between two sections of the Sheet pane.
pub const REGION_GAP: f32 = 12.0;

/// Draws a framed region, gray 900 inside a 1 point gray 800 border, with `add` inside it across the width it is
/// given, and returns what `add` returns. Each card is one, on the Card pane and in the Buttons section of the Sheet
/// pane.
///
/// The region adds no margin and no space between the rows `add` draws.
pub fn region<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::NONE
        .fill(mcs::GRAY_900)
        .stroke(Stroke::new(1.0, mcs::GRAY_800))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

/// Draws a gray 800 hairline across `ui`.
pub fn hairline(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::ZERO, mcs::GRAY_800);
}

/// Draws the title row of a section of the Sheet pane: `title` as a [`title_text`], then a hairline running to what
/// `end` draws at the right edge, such as the section's help.
pub fn region_title(ui: &mut Ui, title: &str, end: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add(egui::Label::new(title_text(ui, title)).selectable(false));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            end(ui);
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter()
                .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, mcs::GRAY_800));
        });
    });
}

/// Paints the keyboard focus outline of a control whose border is `rect` outside the border, in the outline egui draws
/// around a focused button.
pub fn focus_ring(ui: &Ui, rect: Rect) {
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        ui.visuals().widgets.active.bg_stroke,
        StrokeKind::Outside,
    );
}

/// Returns `text` as a small label in gray 400 at the theme's small size, for metadata such as a line number or a fire
/// count.
pub fn small_label(text: impl Into<String>) -> RichText {
    RichText::new(text).small().color(mcs::GRAY_400)
}

/// Adds `text` as a [`small_label`], for a count, a table header or the label of a group, and returns its response.
pub fn caption(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Label::new(small_label(text)).selectable(false))
}

/// Returns the width in points of `text` as a [`caption`] shows it, on one line.
pub fn caption_width(ui: &Ui, text: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            TextStyle::Small.resolve(ui.style()),
            Color32::PLACEHOLDER,
        )
        .size()
        .x
}

/// Returns `text` as the title of a card, a section of the Sheet pane, an overlay or a dialog: the semibold face at the
/// body size in gray 100.
pub fn title_text(ui: &Ui, text: &str) -> RichText {
    let size = TextStyle::Body.resolve(ui.style()).size;
    RichText::new(text)
        .font(crate::init::semibold_font(ui.ctx(), size))
        .color(mcs::GRAY_100)
}

/// Adds `text` as the heading of an overlay or a dialog, a [`title_text`] whose accessible node is a heading named
/// `text`, and returns its response.
pub fn heading_caption(ui: &mut Ui, text: &str) -> Response {
    let label = ui.add(egui::Label::new(title_text(ui, text)).selectable(false));
    ui.ctx().accesskit_node_builder(label.id, |node| {
        node.set_role(egui::accesskit::Role::Heading);
        node.set_label(text);
    });
    label
}

/// Draws `add` inside the frame of an overlay, a menu or a picker, egui's popup frame with its fill, border and shadow,
/// and returns what `add` returns.
///
/// egui fades an area in over its first frames. The overlay sets the opacity back to whole, so the text under it never
/// shows through. The overlay's popup takes [`Frame::NONE`]. The down and up arrow keys move the keyboard focus through
/// the overlay's rows ([`move_through_rows`]).
pub fn overlay<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.set_opacity(1.0);
    let drawn = ui.id().with(OVERLAY_ROWS);
    move_through_rows(ui, drawn);
    let drawing = Id::new(OVERLAY_ROWS);
    let outer = ui.data_mut(|data| {
        data.get_temp_mut_or_default::<Option<Vec<Id>>>(drawing)
            .replace(Vec::new())
    });
    let inner = Frame::popup(ui.style())
        .inner_margin(Margin::same(OVERLAY_PADDING as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner;
    let rows = ui.data_mut(|data| std::mem::replace(data.get_temp_mut_or_default::<Option<Vec<Id>>>(drawing), outer));
    ui.data_mut(|data| data.insert_temp(drawn, rows.unwrap_or_default()));
    inner
}

/// Moves the keyboard focus through the rows an overlay drew in the frame before, kept under `drawn`, when the down or
/// the up arrow key is pressed. The focus moves to the next row or the one before, round from the last row to the
/// first and back, and from outside the overlay, or from a button of the overlay other than a row, into its first or
/// its last row.
///
/// A text field of the overlay keeps the keys while it holds the focus, unless it is a box marked empty by
/// [`mark_empty_box`]. The keys move nothing else in that frame.
fn move_through_rows(ui: &Ui, drawn: Id) {
    let rows: Vec<Id> = ui.data(|data| data.get_temp(drawn)).unwrap_or_default();
    let Some(last) = rows.len().checked_sub(1) else {
        return;
    };
    let (down, up) = ui.input(|input| {
        let plain = input.modifiers.is_none();
        (
            plain && input.key_pressed(Key::ArrowDown),
            plain && input.key_pressed(Key::ArrowUp),
        )
    });
    if down == up {
        return;
    }
    let focused = ui.memory(|memory| memory.focused());
    let index = focused.and_then(|id| rows.iter().position(|row| *row == id));
    // A text field moves its cursor with the keys. Every text field stores its state under its id. An empty box has no
    // text to move through, and passes the keys on to the rows.
    let field_inside = |id: Id| {
        TextEdit::load_state(ui.ctx(), id).is_some()
            && ui
                .ctx()
                .read_response(id)
                .is_some_and(|response| response.layer_id == ui.layer_id())
            && !ui.data(|data| data.get_temp(id.with(EMPTY_BOX))).unwrap_or(false)
    };
    if index.is_none() && focused.is_some_and(field_inside) {
        return;
    }
    let target = match (index, down) {
        (Some(index), true) if index < last => index + 1,
        (Some(index), false) if index > 0 => index - 1,
        (Some(_) | None, true) => 0,
        (Some(_) | None, false) => last,
    };
    ui.memory_mut(|memory| {
        memory.request_focus(rows[target]);
        memory.move_focus(egui::FocusDirection::None);
    });
}

/// Marks the box `id` of an overlay as holding no text while `empty` is set. The down and up arrow keys then move from
/// the box to the overlay's first or last row ([`overlay`]), as from a button of the overlay.
pub fn mark_empty_box(ui: &Ui, id: Id, empty: bool) {
    ui.data_mut(|data| data.insert_temp(id.with(EMPTY_BOX), empty));
}

/// A tooltip of a control that shows while the keyboard holds the control's focus, as well as under the pointer.
pub trait FocusTooltip {
    /// Shows `text` as the control's tooltip under the pointer, as `on_hover_text` does, and while the control holds
    /// the keyboard focus. Returns the control's response.
    ///
    /// The focus counts as the keyboard's while the last press was a key, so a control that takes the focus after a
    /// click shows no tooltip. No tooltip shows while a popup is open.
    fn on_hover_or_focus_text(self, text: &str) -> Self;
}

impl FocusTooltip for Response {
    fn on_hover_or_focus_text(self, text: &str) -> Self {
        if !keyboard_tooltip(&self) {
            return self.on_hover_text(text);
        }
        focus_tooltip(&self, |ui| {
            ui.label(text);
        });
        self
    }
}

/// Returns whether `response`, a control, shows its tooltip for the keyboard: it holds the focus, the last press was a
/// key, and no popup is open.
fn keyboard_tooltip(response: &Response) -> bool {
    // Read on every call, focused or not. The key that moves the focus is pressed a frame before the control holds the
    // focus.
    let key_pressed_last = key_pressed_last(&response.ctx);
    response.has_focus() && key_pressed_last && !Popup::is_any_open(&response.ctx)
}

/// Shows what `add` draws as the tooltip of `response`, a control that holds the keyboard focus.
fn focus_tooltip(response: &Response, add: impl FnOnce(&mut Ui)) {
    // The area takes no click, as egui's own tooltip of plain text does. A click under it reaches the control there.
    Popup::from_response(response)
        .id(response.id.with("henad_focus_tooltip"))
        .kind(egui::PopupKind::Tooltip)
        .gap(4.0)
        .sense(Sense::hover())
        .interactable(false)
        .show(|ui| {
            ui.style_mut().interaction.selectable_labels = false;
            ui.set_max_width(ui.spacing().tooltip_width);
            add(ui);
        });
}

/// Returns whether the last press, in this frame or an earlier one, was a key rather than a pointer button.
fn key_pressed_last(ctx: &egui::Context) -> bool {
    let latest = ctx.input(|input| {
        input.events.iter().rev().find_map(|event| match event {
            egui::Event::Key { pressed: true, .. } => Some(true),
            egui::Event::PointerButton { pressed: true, .. } => Some(false),
            _ => None,
        })
    });
    ctx.data_mut(|data| {
        let last = data.get_temp_mut_or_default::<bool>(Id::new(KEY_PRESSED_LAST));
        if let Some(latest) = latest {
            *last = latest;
        }
        *last
    })
}

/// Puts the keyboard focus back on `anchor` when Escape closed its popup `popup_id` in this frame. `was_open` holds
/// whether the popup was open before the frame drew it.
pub fn escape_to_anchor(ui: &Ui, anchor: &Response, popup_id: Id, was_open: bool) {
    if was_open && !Popup::is_id_open(ui.ctx(), popup_id) && ui.input(|input| input.key_pressed(Key::Escape)) {
        anchor.request_focus();
    }
}

/// Place of an overlay.
enum OverlaySide {
    /// Under its anchor.
    Below,
    /// Over its anchor.
    Above,
    /// At the top of the window, over its anchor.
    WindowTop,
}

/// Returns `popup` placed beside its anchor, and the most height in points the popup takes there.
///
/// The popup opens under its anchor, over it where it does not fit under it, and otherwise on the side with more room,
/// as tall as that room, its items scrolling. When neither side has [`OVERLAY_MIN_ROOM`] points, the popup opens at
/// the top of the window and takes the window's height. The popup fits by its height in the frame before. In its first
/// frame it opens under its anchor with the window's height as its room. The room is kept for [`overlay_room`].
pub fn placed<'a>(ctx: &egui::Context, popup: Popup<'a>) -> (Popup<'a>, f32) {
    let window = ctx.content_rect().shrink(OVERLAY_SCREEN_GAP);
    let Some(anchor) = popup.get_anchor_rect() else {
        return (popup, window.height());
    };
    let below = window.bottom() - anchor.bottom();
    let above = anchor.top() - window.top();
    let (side, room) = match popup.get_expected_size().map(|size| size.y) {
        None => (OverlaySide::Below, window.height()),
        Some(height) if height <= below => (OverlaySide::Below, below),
        Some(height) if height <= above => (OverlaySide::Above, above),
        Some(_) if below.max(above) < OVERLAY_MIN_ROOM => (OverlaySide::WindowTop, window.height()),
        Some(_) if above > below => (OverlaySide::Above, above),
        Some(_) => (OverlaySide::Below, below),
    };
    let popup = match side {
        OverlaySide::Below => popup
            .align(RectAlign::BOTTOM_START)
            .align_alternatives(&[RectAlign::BOTTOM_END]),
        OverlaySide::Above => popup
            .align(RectAlign::TOP_START)
            .align_alternatives(&[RectAlign::TOP_END]),
        // Under a line along the top of the window, which the popup then covers down to the bottom.
        OverlaySide::WindowTop => popup
            .anchor(Rect::from_x_y_ranges(anchor.x_range(), window.top()..=window.top()))
            .align(RectAlign::BOTTOM_START)
            .align_alternatives(&[RectAlign::BOTTOM_END]),
    };
    let id = popup.get_id().with(OVERLAY_ROOM);
    ctx.data_mut(|data| data.insert_temp(id, room));
    (popup, room)
}

/// Returns the most height in points the overlay `ui` draws in takes, as [`placed`] set it, or the window's height for
/// an overlay that was not placed.
pub fn overlay_room(ui: &Ui) -> f32 {
    let id = ui.layer_id().id.with(OVERLAY_ROOM);
    ui.data(|data| data.get_temp(id))
        .unwrap_or_else(|| ui.ctx().content_rect().shrink(OVERLAY_SCREEN_GAP).height())
}

/// Draws `add`, the items of an overlay under its title, in a vertical scroll area as tall as the overlay's room leaves
/// for them ([`overlay_room`]), and returns what `add` returns. A short list takes its own height and shows no scroll
/// bar.
///
/// The overlay then fits in its room, and the wheel brings its last item into view.
pub fn overlay_items<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    // The title above the cursor, and the overlay's padding and border at both ends, take the rest of the room.
    let above = ui.cursor().top() - ui.max_rect().top() + OVERLAY_PADDING + 1.0;
    let below = OVERLAY_PADDING + 1.0;
    let room = (overlay_room(ui) - above - below).max(3.0 * OVERLAY_ROW);
    // The popup's area holds the size of its first frame as the room it offers. The scroll area takes `room`
    // whatever the area offers, and shrinks to a shorter list.
    egui::ScrollArea::vertical()
        .max_height(room)
        .min_scrolled_height(room)
        .show(ui, add)
        .inner
}

/// Draws the title of an overlay as a heading in [`title_text`], with a hairline under it across the overlay, and
/// returns the heading's response.
pub fn overlay_title(ui: &mut Ui, title: &str) -> Response {
    ui.add_space(2.0);
    let heading = heading_caption(ui, title);
    ui.add_space(4.0);
    overlay_rule(ui);
    heading
}

/// Draws the label of a group of an overlay's items as a [`caption`], with a hairline above it across the overlay, and
/// returns the label's response.
///
/// A group right under the overlay's title (`under_title`) shares the hairline under the title and draws none of its
/// own.
pub fn overlay_group(ui: &mut Ui, label: &str, under_title: bool) -> Response {
    if !under_title {
        ui.add_space(4.0);
        overlay_rule(ui);
    }
    ui.add_space(4.0);
    let caption = caption(ui, label);
    ui.add_space(2.0);
    caption
}

/// Draws `add`, a group of the items of a menu or a picker, in a child `Ui` whose accessible node is a menu labelled by
/// the node `label`, and returns what `add` returns.
///
/// `label` is the group's label from [`overlay_group`], or for a group without one the overlay's title from
/// [`overlay_title`] or the button that opens the overlay. The menu takes its name from that node and holds no name of
/// its own.
pub fn menu_group<R>(ui: &mut Ui, label: Id, add: impl FnOnce(&mut Ui) -> R) -> R {
    let group = ui.scope(|ui| (add(ui), ui.unique_id()));
    let (inner, id) = group.inner;
    let rect = group.response.rect;
    ui.ctx().accesskit_node_builder(id, |node| {
        node.set_role(egui::accesskit::Role::Menu);
        node.push_labelled_by(label.accesskit_id());
        node.set_bounds(accessible_bounds(rect));
    });
    inner
}

/// Adds `item` as a row of a menu or a picker, an [`overlay_item`] of role `MenuItem`, and returns its response. A
/// [`menu_group`] holds it.
pub fn menu_item(ui: &mut Ui, item: &OverlayItem<'_>) -> Response {
    let response = overlay_item(ui, item);
    ui.ctx()
        .accesskit_node_builder(response.id, |node| node.set_role(egui::accesskit::Role::MenuItem));
    response
}

/// Draws a hairline across an overlay, through its padding at both sides.
fn overlay_rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(1.0), 1.0), Sense::hover());
    let across = ui.max_rect().union(rect).x_range().expand(OVERLAY_PADDING);
    ui.painter()
        .hline(across, rect.center().y, Stroke::new(1.0, mcs::GRAY_800));
}

// The symbols of the types of values

/// Id of the persisted setting that shows a symbol of its type before each value, property and parameter of a sheet.
const SHOW_TYPE_SYMBOLS_ID: &str = "henad_show_type_symbols";

/// Width in points of the column a type's symbol takes, with the gap after it.
const SYMBOL_COLUMN: f32 = 18.0;

/// Type of a value, a property or a parameter of a sheet, which a symbol before its name shows.
///
/// A number and a vector take the symbols the Statistics tab shows for a scalar and a vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeSymbol {
    /// A number.
    Number,
    /// An integer.
    Integer,
    /// A Boolean, yes or no.
    Boolean,
    /// A vector.
    Vector,
    /// An agent's place in the world.
    Position,
    /// The direction an agent last moved in.
    Heading,
    /// One of a few named states.
    States,
    /// A cell of the grid or the ground.
    Cell,
}

impl TypeSymbol {
    /// Returns the symbol of a property of type `property_type`.
    pub fn of_property(property_type: PropertyType) -> Self {
        match property_type {
            PropertyType::States => Self::States,
            PropertyType::WholeNumber => Self::Integer,
            PropertyType::YesNo => Self::Boolean,
            PropertyType::Arrow => Self::Vector,
            PropertyType::Position => Self::Position,
            PropertyType::Heading => Self::Heading,
            _ => Self::Number,
        }
    }

    /// Returns the symbol of a parameter of type `parameter_type`.
    pub fn of_parameter(parameter_type: ParameterType) -> Self {
        if parameter_type == ParameterType::WholeNumber {
            Self::Integer
        } else {
            Self::Number
        }
    }

    /// Returns the symbol of a value of the kind `kind`, `None` for a kind no symbol shows, such as a list.
    pub fn of_kind(kind: ValueKind) -> Option<Self> {
        match kind {
            ValueKind::YesNo => Some(Self::Boolean),
            ValueKind::Number => Some(Self::Number),
            ValueKind::Arrow => Some(Self::Vector),
            ValueKind::Place => Some(Self::Cell),
            ValueKind::State => Some(Self::States),
            _ => None,
        }
    }

    /// Glyph of the symbol in the icon font.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Number => MDI_CIRCLE_SMALL,
            Self::Integer => MDI_POUND,
            Self::Boolean => MDI_TOGGLE_SWITCH_OUTLINE,
            Self::Vector => MDI_ARROW_TOP_RIGHT_THIN,
            Self::Position => MDI_MAP_MARKER_OUTLINE,
            Self::Heading => MDI_COMPASS_OUTLINE,
            Self::States => MDI_LABEL_OUTLINE,
            Self::Cell => MDI_SQUARE_OUTLINE,
        }
    }

    /// Name of the type, the symbol's tooltip and accessible name, such as "Vector".
    pub fn name(self) -> &'static str {
        match self {
            Self::Number => NUMBER_KIND,
            Self::Integer => INTEGER_KIND,
            Self::Boolean => YES_NO_KIND,
            Self::Vector => ARROW_KIND,
            Self::Position => POSITION_KIND,
            Self::Heading => HEADING_KIND,
            Self::States => STATES_KIND,
            Self::Cell => CELL_KIND,
        }
    }
}

/// Returns whether the sheet editor shows the symbol of its type before each value, property and parameter, a setting
/// the app keeps from one run to the next. It is on until turned off.
pub fn type_symbols_shown(ctx: &egui::Context) -> bool {
    ctx.data_mut(|data| *data.get_persisted_mut_or(Id::new(SHOW_TYPE_SYMBOLS_ID), true))
}

/// Returns the width in points a type's symbol takes before a name of the Sheet pane: none while the symbols are
/// hidden.
pub fn type_symbol_width(ctx: &egui::Context) -> f32 {
    if type_symbols_shown(ctx) { SYMBOL_COLUMN } else { 0.0 }
}

/// Shows or hides the symbols of [`type_symbols_shown`], as `shown` says.
pub fn show_type_symbols(ctx: &egui::Context, shown: bool) {
    ctx.data_mut(|data| data.insert_persisted(Id::new(SHOW_TYPE_SYMBOLS_ID), shown));
}

/// Adds the weak symbol of `symbol` before a name in a row of the Sheet pane, with the type's name as its tooltip,
/// named as `type_symbol_name` writes it, while [`type_symbols_shown`] holds.
pub fn type_symbol_ui(ui: &mut Ui, symbol: TypeSymbol) {
    if !type_symbols_shown(ui.ctx()) {
        return;
    }
    let (rect, response) = ui.allocate_exact_size(vec2(SYMBOL_COLUMN, SYMBOL_LINE), Sense::hover());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Image, true, type_symbol_name(symbol.name())));
    paint_symbol(ui.painter(), rect.left_center(), symbol, ui.visuals().weak_text_color());
    response.on_hover_text(symbol.name());
}

/// Paints the glyph of `symbol` in `color` with its left edge at `left`, centred on its height.
fn paint_symbol(painter: &Painter, left: Pos2, symbol: TypeSymbol, color: Color32) {
    painter.text(
        left,
        egui::Align2::LEFT_CENTER,
        symbol.glyph(),
        FontId::proportional(SYMBOL_SIZE),
        color,
    );
}

/// Size in points of a type's symbol.
const SYMBOL_SIZE: f32 = 14.0;

/// Height in points of the line a type's symbol is centred on in the Sheet pane, the pane's line of a table row.
const SYMBOL_LINE: f32 = 22.0;

/// One item of an overlay.
#[derive(Debug, Clone, Copy, Default)]
pub struct OverlayItem<'a> {
    /// Words of the item. They name it too, unless `name` gives a name of its own.
    pub label: &'a str,
    /// Accessible name of the item when it differs from its words, `None` otherwise.
    pub name: Option<&'a str>,
    /// Grey words under the item, `None` for none.
    pub detail: Option<&'a str>,
    /// Whether the grey words are the item's accessible description. Otherwise they stay out of the tree.
    pub described: bool,
    /// Whether the item is the current value of what the overlay sets. It then takes the tinted fill and a check.
    pub current: bool,
    /// Colour of a state's swatch before the words, `None` for an item that is no state.
    pub swatch: Option<Color32>,
    /// Symbol of the type of the item's value before its words, `None` for an item with no type or while the symbols
    /// are hidden. Its name is the tooltip of the symbol.
    pub symbol: Option<TypeSymbol>,
    /// Whether an item with no symbol leaves the symbol's column empty, to line up with items of the same list that
    /// have one.
    pub symbol_space: bool,
    /// Reason an option of a select cannot be chosen, `None` for an option that can. [`select_list`] draws a refused
    /// option weak, with the reason as its tooltip.
    pub refusal: Option<&'a str>,
}

/// Adds `item` as a full-width row of an overlay and returns its response.
///
/// The row takes the theme's hover fill and hover text under the pointer and under keyboard focus, and the focus outline
/// inside its edge while it holds the focus. A current value takes the theme's selection fill, its words in the
/// selection's text colour after a check.
/// Inside a disabled `Ui` the words are weak and the row takes no hover and no click. An enabled row is one of the rows
/// that the down and up arrow keys move through, and a row that gains the focus outside its scroll area's view scrolls
/// into view.
pub fn overlay_item(ui: &mut Ui, item: &OverlayItem<'_>) -> Response {
    let enabled = ui.is_enabled();
    let body = TextStyle::Body.resolve(ui.style());
    let swatch_width = item.swatch.map_or(0.0, |_| CHIP_SWATCH + CHIP_GAP);
    let symbol_width = if item.symbol.is_some() || item.symbol_space {
        SYMBOL_COLUMN
    } else {
        0.0
    };
    let indent = MARK_COLUMN + symbol_width + swatch_width;
    let label = ui
        .painter()
        .layout_no_wrap(item.label.to_owned(), body, Color32::PLACEHOLDER);
    let label_size = label.size();
    let wrap = (ui.available_width() - indent).max(160.0);
    let weak = ui.visuals().weak_text_color();
    let detail = item.detail.map(|detail| {
        ui.painter()
            .layout(detail.to_owned(), FontId::proportional(OVERLAY_DETAIL), weak, wrap)
    });
    let detail_size = detail.as_ref().map_or(Vec2::ZERO, |galley| galley.size());
    let content = label_size.y + detail_size.y;
    let height = (content + 6.0).max(OVERLAY_ROW);
    let (id, rect) = ui.allocate_space(vec2(indent + label_size.x.max(detail_size.x), height));
    // The row runs across the overlay, through the overlay's padding at both sides.
    let row = Rect::from_x_y_ranges(
        ui.max_rect().union(rect).x_range().expand(OVERLAY_PADDING),
        rect.y_range(),
    );
    let response = ui.interact(row, id, Sense::click());
    let name = item.name.unwrap_or(item.label);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, name));
    if enabled {
        ui.data_mut(|data| {
            if let Some(rows) = data.get_temp_mut_or_default::<Option<Vec<Id>>>(Id::new(OVERLAY_ROWS)) {
                rows.push(response.id);
            }
        });
    }
    // The symbol of the item's type names the type under the pointer.
    let symbol_rect = Rect::from_min_size(pos2(rect.left() + MARK_COLUMN, row.top()), vec2(symbol_width, height));
    let response = match item.symbol {
        Some(symbol)
            if enabled
                && response
                    .hover_pos()
                    .is_some_and(|pointer| symbol_rect.contains(pointer)) =>
        {
            response.on_hover_text(symbol.name())
        }
        _ => response,
    };
    // Only a row its overlay's scroll area hides scrolls. A row in view would scroll the pane under the overlay.
    let clip = ui.clip_rect();
    if response.gained_focus() && (response.rect.top() < clip.top() || response.rect.bottom() > clip.bottom()) {
        response.scroll_to_me(None);
    }
    if let Some(detail) = item.detail.filter(|_| item.described) {
        ui.ctx()
            .accesskit_node_builder(response.id, |node| node.set_description(detail));
    }
    if ui.is_rect_visible(row) {
        let active = enabled && (response.hovered() || response.has_focus());
        let visuals = ui.visuals();
        let text_color = if !enabled {
            visuals.weak_text_color()
        } else if item.current {
            visuals.selection.stroke.color
        } else if active {
            visuals.widgets.hovered.text_color()
        } else {
            visuals.widgets.inactive.text_color()
        };
        if let Some(fill) = list_row_fill(visuals, enabled && item.current, active) {
            ui.painter().rect_filled(row, CornerRadius::ZERO, fill);
        }
        let top = row.top() + (height - content) / 2.0;
        let line_center = top + label_size.y / 2.0;
        paint_list_row_marks(ui, row, item.current, response.has_focus(), line_center);
        let painter = ui.painter();
        let mut x = rect.left() + MARK_COLUMN;
        if let Some(symbol) = item.symbol {
            paint_symbol(painter, pos2(x, line_center), symbol, weak);
        }
        x += symbol_width;
        if let Some(color) = item.swatch {
            swatch(painter, pos2(x + CHIP_SWATCH / 2.0, line_center), color);
            x += swatch_width;
        }
        painter.galley(pos2(x, top), label, text_color);
        if let Some(detail) = detail {
            painter.galley(pos2(x, top + label_size.y), detail, weak);
        }
    }
    response
}

/// Adds a state's swatch of the colour `color` beside the words of a row: a small square inside a gray 700 outline.
pub fn swatch_ui(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(CHIP_SWATCH, CHIP_HEIGHT), Sense::hover());
    swatch(ui.painter(), rect.center(), color);
}

/// Paints a state's swatch of the colour `color` centred on `center`: a small square inside a gray 700 outline.
fn swatch(painter: &egui::Painter, center: Pos2, color: Color32) {
    let square = Rect::from_center_size(center, Vec2::splat(CHIP_SWATCH));
    painter.rect_filled(square, CornerRadius::ZERO, color);
    painter.rect_stroke(
        square,
        CornerRadius::ZERO,
        Stroke::new(1.0, mcs::GRAY_700),
        StrokeKind::Inside,
    );
}

/// Returns whether the popup of the control `id` is open.
fn picker_open(ui: &Ui, id: Option<Id>) -> bool {
    id.is_some_and(|id| Popup::is_id_open(ui.ctx(), id.with("picker")))
}

/// Whether a button shows its frame while nothing happens to it, as a framed button does, or only under the pointer,
/// on focus and while selected, as a frameless one does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Framed {
    /// The frame shows in every state.
    Always,
    /// The frame shows only under the pointer, on focus, while pressed or while selected.
    WhenActive,
}

/// Paints the frame egui gives a button whose border is `rect` in the state of `response`, and returns the theme's
/// visuals of that state, whose text colour the button's words take.
///
/// The frame is the visuals' fill inside their outline, grown by their expansion, as egui's `Button` paints it. While
/// `selected` (a control whose popup is open) it takes the selection fill and text colour. `fill` stands in for the
/// visuals' fill of a framed button that is not selected, as a chip's fill shows its value.
fn paint_button(
    ui: &Ui,
    rect: Rect,
    response: &Response,
    selected: bool,
    framed: Framed,
    fill: Option<Color32>,
) -> egui::style::WidgetVisuals {
    let visuals = ui.style().interact_selectable(response, selected);
    let active = selected || response.hovered() || response.has_focus() || response.is_pointer_button_down_on();
    if framed == Framed::Always || active {
        let fill = fill.filter(|_| !selected).unwrap_or(visuals.weak_bg_fill);
        ui.painter().rect(
            rect.expand(visuals.expansion),
            visuals.corner_radius,
            fill,
            visuals.bg_stroke,
            StrokeKind::Inside,
        );
    }
    visuals
}

/// Returns the width in points of a text control showing `text`, as [`text_control`] adds it.
pub fn text_control_width(ui: &Ui, text: &str) -> f32 {
    let font = TextStyle::Body.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
        .size()
        .x
        + 2.0 * ui.spacing().button_padding.x
}

/// Adds a text control showing `text`, named `name`, under the widget id `id` when one is given, and returns its
/// response: "always", "for each", the "+" after a list and the "×" that deletes a row.
///
/// It is a frameless button: its words weak with no frame, and egui's button frame under the pointer, on focus and,
/// as a selected button, while its picker is open. Its hit area is as high as the row of the sentence `row`. It is
/// laid out as one unbroken piece, so a row that has no room for it moves it to the next row whole.
pub fn text_control(ui: &mut Ui, id: Option<Id>, text: &str, name: &str, row: f32) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let width = text_control_width(ui, text);
    let (rect, response) = allocate_control(ui, id, vec2(width, row));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let boxed = Rect::from_center_size(rect.center(), vec2(width, CHIP_HEIGHT));
        let open = picker_open(ui, id);
        let visuals = paint_button(ui, boxed, &response, open, Framed::WhenActive, None);
        let idle = !open && !response.hovered() && !response.has_focus();
        let color = if idle {
            ui.visuals().weak_text_color()
        } else {
            visuals.text_color()
        };
        ui.painter().galley(
            Pos2::new(
                boxed.left() + ui.spacing().button_padding.x,
                boxed.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }
    response
}

/// Adds an inline select showing `words` and a chevron, named `name`, under the widget id `id` when one is given, and
/// returns its response.
///
/// A filled select looks as egui's combo box does: the theme's button frame, its words and chevron in the theme's text
/// colour, and the open frame while its options are open. An `empty` select is an empty slot as a blank is: its words
/// weak inside a faded outline ([`slot_outline`]), and the selection fill while its options are open.
pub fn select_control(ui: &mut Ui, id: Option<Id>, words: &str, name: &str, empty: bool) -> Response {
    select_within(ui, id, words, name, empty, f32::INFINITY)
}

/// Adds an inline select as [`select_control`] does, at most `max_width` wide.
///
/// Words too long for that width end in an ellipsis, and the whole words show as the select's tooltip. Inside a
/// disabled `Ui` the words are weak.
pub fn select_within(ui: &mut Ui, id: Option<Id>, words: &str, name: &str, empty: bool, max_width: f32) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let enabled = ui.is_enabled();
    let weak = ui.visuals().weak_text_color();
    let painter = ui.painter();
    let chevron = painter.layout_no_wrap(
        MDI_CHEVRON_DOWN.to_owned(),
        FontId::proportional(SMALL_ICON),
        Color32::PLACEHOLDER,
    );
    let frame = CHIP_PADDING + CHIP_GAP + chevron.size().x + 3.0;
    let mut job = egui::text::LayoutJob::simple_singleline(words.to_owned(), font, Color32::PLACEHOLDER);
    job.wrap = egui::text::TextWrapping::truncate_at_width((max_width - frame).max(0.0));
    let text = painter.layout_job(job);
    let width = frame + text.size().x;
    let (rect, response) = allocate_control(ui, id, vec2(width, CHIP_HEIGHT));
    let response = if text.elided {
        response.on_hover_or_focus_text(words)
    } else {
        response
    };
    response.widget_info(|| WidgetInfo {
        current_text_value: (!words.is_empty()).then(|| words.to_owned()),
        ..WidgetInfo::labeled(
            if words.is_empty() {
                WidgetType::Button
            } else {
                WidgetType::ComboBox
            },
            enabled,
            name,
        )
    });
    if ui.is_rect_visible(rect) {
        let open = picker_open(ui, id);
        let color = if empty && !open {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::ZERO,
                slot_outline(response.hovered()),
                StrokeKind::Inside,
            );
            if response.has_focus() {
                focus_ring(ui, rect);
            }
            weak
        } else if empty {
            paint_button(ui, rect, &response, true, Framed::Always, None).text_color()
        } else {
            let visuals = if open {
                ui.visuals().widgets.open
            } else {
                *ui.style().interact(&response)
            };
            ui.painter().rect(
                rect.expand(visuals.expansion),
                visuals.corner_radius,
                visuals.weak_bg_fill,
                visuals.bg_stroke,
                StrokeKind::Inside,
            );
            visuals.text_color()
        };
        let color = if enabled { color } else { weak };
        let painter = ui.painter();
        let left = Pos2::new(rect.left() + CHIP_PADDING, rect.center().y - text.size().y / 2.0);
        let chevron_left = left.x + text.size().x + CHIP_GAP;
        painter.galley(left, text, color);
        painter.galley(
            Pos2::new(chevron_left, rect.center().y - chevron.size().y / 2.0),
            chevron,
            color,
        );
    }
    response
}

/// Draws the list of options of a select under `anchor` while the popup `popup_id` is open, one row of `items` per
/// option, and returns the index of the option chosen.
///
/// A click on `anchor` opens and closes the list, and a choice or a click outside closes it. The list is an overlay. An
/// option with a refusal is weak and takes no click, and shows its refusal under the pointer.
pub fn select_list(ui: &Ui, anchor: &Response, popup_id: Id, items: &[OverlayItem<'_>]) -> Option<usize> {
    let was_open = Popup::is_id_open(ui.ctx(), popup_id);
    if anchor.clicked() {
        Popup::toggle_id(ui.ctx(), popup_id);
    }
    let (popup, _) = placed(
        ui.ctx(),
        Popup::from_response(anchor)
            .id(popup_id)
            .open_memory(None)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .frame(Frame::NONE),
    );
    let popup = popup.show(|ui| {
        overlay(ui, |ui| {
            let mut chosen = None;
            for (index, item) in items.iter().enumerate() {
                let response = ui
                    .add_enabled_ui(item.refusal.is_none(), |ui| overlay_item(ui, item))
                    .inner;
                match item.refusal {
                    Some(refusal) => {
                        response.on_disabled_hover_text(refusal);
                    }
                    None if response.clicked() => chosen = Some(index),
                    None => {}
                }
            }
            if chosen.is_some() {
                ui.close();
            }
            chosen
        })
    });
    escape_to_anchor(ui, anchor, popup_id, was_open);
    popup?.inner
}

/// Adds the chevron after a condition word, named `name` under the widget id `id`, and returns its response.
///
/// It is a frameless button, a chevron close after the word with egui's button frame, as high as a chip, under the
/// pointer, on focus and, as a selected button, while "Choose when" is open. Its hit area is as high as the row of the
/// sentence `row`.
pub fn arrow_control(ui: &mut Ui, id: Id, name: &str, row: f32) -> Response {
    let (rect, response) = allocate_control(ui, Some(id), vec2(ARROW_WIDTH, row));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let boxed = Rect::from_center_size(
            pos2(rect.left() + 2.0 + ARROW_SIDE / 2.0, rect.center().y),
            vec2(ARROW_SIDE, CHIP_HEIGHT),
        );
        let visuals = paint_button(
            ui,
            boxed,
            &response,
            picker_open(ui, Some(id)),
            Framed::WhenActive,
            None,
        );
        ui.painter().text(
            boxed.center(),
            egui::Align2::CENTER_CENTER,
            MDI_CHEVRON_DOWN,
            FontId::proportional(SMALL_ICON),
            visuals.text_color(),
        );
    }
    response
}

/// Adds the menu button of a line, a dots glyph centred in `rect`, named `name` under the widget id `id`, and returns
/// its response. The menu is the popup `popup_id`.
///
/// It is a frameless button: weak dots with no frame, and egui's button frame, as high as a chip, under the pointer, on
/// focus and, as a selected button, while its menu is open.
pub fn menu_button(ui: &Ui, id: Id, popup_id: Id, name: &str, rect: Rect) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let open = Popup::is_id_open(ui.ctx(), popup_id);
        let boxed = Rect::from_center_size(rect.center(), vec2(rect.width() - 4.0, CHIP_HEIGHT));
        let visuals = paint_button(ui, boxed, &response, open, Framed::WhenActive, None);
        let idle = !open && !response.hovered() && !response.has_focus();
        let color = if idle {
            ui.visuals().weak_text_color()
        } else {
            visuals.text_color()
        };
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            MDI_DOTS_HORIZONTAL,
            TextStyle::Body.resolve(ui.style()),
            color,
        );
    }
    response
}

/// Returns the hue of the bar of a line of the group `button`, the hue's 500 step. The square on that add button
/// shows it too.
///
/// This is the one place a hue names a kind of line in place of a state.
pub fn kind_hue(button: AddButton) -> Color32 {
    match button {
        AddButton::LookAround => mcs::BLUE_500,
        AddButton::Chance => mcs::PURPLE_500,
        AddButton::YesOrNo => mcs::YELLOW_500,
        AddButton::Calculate => mcs::GRAY_500,
        AddButton::Change => mcs::GREEN_500,
        _ => mcs::GRAY_100,
    }
}

/// Paints the bar of a line of the group `group` at the left edge of `rect`, [`KIND_BAR_WIDTH`] wide and as tall as
/// `rect`, in the group's [`kind_hue`].
pub fn kind_bar(painter: &Painter, rect: Rect, group: AddButton) {
    let bar = Rect::from_min_size(rect.min, vec2(KIND_BAR_WIDTH, rect.height()));
    painter.rect_filled(bar, CornerRadius::ZERO, kind_hue(group));
}

/// Adds the button that adds a row, a line of a card or a row of a table, showing `words` and named `name`, and
/// returns its response: the words weak inside a faded outline ([`slot_outline`]), with the square of the
/// [`kind_hue`] of the group `kind` before them on a card's add buttons.
///
/// Under the pointer and while the menu it opens is `open`, the outline is brighter and the words take the text colour.
pub fn add_row_button(ui: &mut Ui, words: &str, name: &str, kind: Option<AddButton>, open: bool) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(words.to_owned(), font, Color32::PLACEHOLDER);
    let square = kind.map_or(0.0, |_| ADD_SQUARE + 5.0);
    let size = vec2(galley.size().x + square + 12.0, OVERLAY_ROW - 2.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), name));
    if ui.is_rect_visible(rect) {
        let bright = open || response.hovered();
        let color = if bright {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let painter = ui.painter();
        painter.rect_stroke(rect, CornerRadius::ZERO, slot_outline(bright), StrokeKind::Inside);
        let mut x = rect.left() + 6.0;
        if let Some(kind) = kind {
            let square = Rect::from_min_size(pos2(x, rect.center().y - ADD_SQUARE / 2.0), Vec2::splat(ADD_SQUARE));
            painter.rect_filled(square, CornerRadius::ZERO, kind_hue(kind));
            x += ADD_SQUARE + 5.0;
        }
        painter.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
        if response.has_focus() {
            focus_ring(ui, rect);
        }
    }
    response
}

/// Adds the small button of a note, showing `words` and named `name`, and returns its response: egui's small button.
pub fn note_button(ui: &mut Ui, words: &str, name: &str) -> Response {
    crate::ui::sweep::layout::add_button(ui, true, Button::new(words).small(), name)
}

/// Adds one segment of a segmented toggle showing `words`, selected while `selected` holds, and returns its response:
/// egui's selectable button, the selection fill while on.
///
/// The segment is a button on the keyboard path, which Space or Enter presses.
pub fn toggle_label(ui: &mut Ui, words: &str, selected: bool) -> Response {
    ui.add(Button::selectable(selected, words))
}

/// Paints the hairline between two segments of a segmented toggle, as high as their words.
pub fn segment_rule(ui: &mut Ui) {
    let height = TextStyle::Body.resolve(ui.style()).size;
    vertical_hairline(ui, height);
}

/// Paints a vertical gray 800 hairline `height` points high, as between two groups of buttons.
pub fn vertical_hairline(ui: &mut Ui, height: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, height), Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1.0, mcs::GRAY_800));
}

/// Adds the mark beside the name of a model with unsaved changes, a small dot in the weak text colour named
/// [`UNSAVED_MARK`] with [`UNSAVED_MARK_TOOLTIP`] as its tooltip, and returns its response.
pub fn unsaved_mark(ui: &mut Ui) -> Response {
    let dot = RichText::new(MDI_CIRCLE_MEDIUM).color(ui.visuals().weak_text_color());
    let mark = ui.add(egui::Label::new(dot).selectable(false).sense(Sense::hover()));
    set_name(ui, mark.id, UNSAVED_MARK);
    mark.on_hover_text(UNSAVED_MARK_TOOLTIP)
}

/// Adds an information notice of `text`, as [`Notice`] draws it.
pub fn notice_ui(ui: &mut Ui, text: &str) {
    Notice::new(NoticeKind::Info, text).show(ui);
}

/// Kind of a notice, which picks its icon and the icon's hue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    /// Information, blue 500 with an information icon.
    Info,
    /// A warning, orange 500 with a triangle.
    Warning,
    /// An error, red 500 with an exclamation mark.
    Error,
}

impl NoticeKind {
    /// Returns the kind of a problem of `severity`: an error for an error, a warning otherwise.
    pub fn of(severity: henad_core::diagnostic::DiagnosticSeverity) -> Self {
        if severity == henad_core::diagnostic::DiagnosticSeverity::Error {
            Self::Error
        } else {
            Self::Warning
        }
    }

    /// Hue of the notice's icon, at step 500.
    fn hue(self) -> Color32 {
        match self {
            Self::Info => mcs::BLUE_500,
            Self::Warning => mcs::ORANGE_500,
            Self::Error => mcs::RED_500,
        }
    }

    /// Icon before the notice's text.
    fn icon(self) -> &'static str {
        match self {
            Self::Info => MDI_INFORMATION,
            Self::Warning => MDI_ALERT,
            Self::Error => MDI_ALERT_CIRCLE,
        }
    }
}

/// A notice: a row with an icon in the hue of its kind before its text in the text colour, and any small buttons after
/// the text.
///
/// The text reads as one label, the icon, a space and the text. Weak notes under the text sit inside the row.
#[derive(Debug, Clone, Copy)]
pub struct Notice<'a> {
    /// Kind of the notice.
    kind: NoticeKind,
    /// Text of the notice.
    text: &'a str,
    /// Whether the text is in the mono face, as a problem placed at a line and a column of a text.
    mono: bool,
    /// Whether the text can be selected and copied.
    selectable: bool,
    /// Whether a click on the text is reported, as on a problem that moves to its place.
    clickable: bool,
    /// Weak lines under the text.
    notes: &'a [String],
}

impl<'a> Notice<'a> {
    /// Returns a notice of `kind` showing `text`.
    pub fn new(kind: NoticeKind, text: &'a str) -> Self {
        Self {
            kind,
            text,
            mono: false,
            selectable: false,
            clickable: false,
            notes: &[],
        }
    }

    /// Sets the text in the mono face.
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// Lets the text be selected and copied.
    pub fn selectable(mut self) -> Self {
        self.selectable = true;
        self
    }

    /// Reports a click on the text through the response [`Self::show`] returns.
    pub fn clickable(mut self) -> Self {
        self.clickable = true;
        self
    }

    /// Adds `notes` as weak lines under the text.
    pub fn notes(mut self, notes: &'a [String]) -> Self {
        self.notes = notes;
        self
    }

    /// Adds the notice across `ui`, and returns the response of its text.
    pub fn show(self, ui: &mut Ui) -> Response {
        self.show_with(ui, |_| ()).0
    }

    /// Adds the notice across `ui` with what `end` adds after its text, such as a small button, and returns the
    /// response of its text and what `end` returns.
    pub fn show_with<R>(self, ui: &mut Ui, end: impl FnOnce(&mut Ui) -> R) -> (Response, R) {
        let body = TextStyle::Body.resolve(ui.style());
        let font = if self.mono {
            FontId::monospace(body.size - 1.0)
        } else {
            body.clone()
        };
        let mut job = egui::text::LayoutJob::default();
        job.append(
            self.kind.icon(),
            0.0,
            egui::TextFormat {
                font_id: body,
                color: self.kind.hue(),
                ..Default::default()
            },
        );
        job.append(
            &format!(" {}", self.text),
            0.0,
            egui::TextFormat {
                font_id: font,
                color: ui.visuals().text_color(),
                ..Default::default()
            },
        );
        let mut label = egui::Label::new(job).wrap().selectable(self.selectable);
        if self.clickable {
            label = label.sense(Sense::click());
        }
        Frame::NONE
            .inner_margin(Margin {
                left: 8,
                right: 4,
                top: 3,
                bottom: 3,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let inner = ui
                    .horizontal_wrapped(|ui| {
                        let text = ui.add(label);
                        (text, end(ui))
                    })
                    .inner;
                for note in self.notes {
                    ui.add(egui::Label::new(RichText::new(note).weak()).wrap());
                }
                inner
            })
            .inner
    }
}

// Dialogs

/// Margin in points between a dialog's border and what it holds.
const DIALOG_MARGIN: i8 = 12;

/// Space in points a dialog whose list scrolls leaves between its frame and the window's top and bottom edges.
const DIALOG_SCREEN_GAP: f32 = 8.0;

/// Returns the frame of a dialog: egui's window frame with its fill, border and shadow, and 12 points of margin.
pub fn dialog_frame(ctx: &egui::Context) -> Frame {
    Frame::window(&ctx.global_style()).inner_margin(Margin::same(DIALOG_MARGIN))
}

/// Draws `add`, the list of a dialog, in a vertical scroll area as tall as the window leaves for it, and returns what
/// `add` returns. `below` is the height in points of what the dialog draws under the list. A short list takes its own
/// height and shows no scroll bar.
///
/// The dialog then fits in the window, with its buttons in view.
pub fn dialog_list<R>(ui: &mut Ui, below: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    let above = ui.cursor().top() - ui.max_rect().top();
    let frame = 2.0 * (f32::from(DIALOG_MARGIN) + 1.0);
    let room =
        (ui.ctx().content_rect().height() - 2.0 * DIALOG_SCREEN_GAP - frame - above - below).max(3.0 * OVERLAY_ROW);
    // The modal's area holds the size of its first frame as the room it offers, as an overlay's popup does.
    egui::ScrollArea::vertical()
        .max_height(room)
        .min_scrolled_height(room)
        .show(ui, add)
        .inner
}

/// Starts a modal dialog `width` points wide named `name` with the role of a dialog, and draws its title: `title` as a
/// heading in [`title_text`] with a gray 800 hairline under it across the dialog.
///
/// The dialog is marked modal. The `Modal` that draws it blocks the input to the app behind it.
pub fn dialog_title(ui: &mut Ui, name: &str, title: &str, width: f32) {
    ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
        node.set_role(egui::accesskit::Role::Dialog);
        node.set_label(name);
        node.set_modal();
    });
    ui.set_width(width);
    heading_caption(ui, title);
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    let across = rect.x_range().expand(f32::from(DIALOG_MARGIN));
    ui.painter()
        .hline(across, rect.center().y, Stroke::new(1.0, mcs::GRAY_800));
    ui.add_space(8.0);
}

/// Look of a button of a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogButtonKind {
    /// The theme's button, as Cancel.
    Plain,
    /// The primary action, blue 700 under gray 50.
    Primary,
    /// An action that discards work, red 900 under red 200.
    Destructive,
}

/// One button of a dialog.
#[derive(Debug, Clone, Copy)]
pub struct DialogButton<'a> {
    /// Words of the button. They also name it.
    pub text: &'a str,
    /// Look of the button.
    pub kind: DialogButtonKind,
    /// Reason the button is disabled, its tooltip, `None` while it is enabled.
    pub refusal: Option<&'a str>,
}

impl<'a> DialogButton<'a> {
    /// Returns an enabled button of `kind` showing `text`.
    pub fn new(text: &'a str, kind: DialogButtonKind) -> Self {
        Self {
            text,
            kind,
            refusal: None,
        }
    }
}

/// Adds the buttons of a dialog in one row ending at its right edge, in the order of `buttons`, which puts the primary
/// last, and returns the index of the one pressed.
///
/// Enter presses the last button, the primary, unless another button took the key or the dialog opened on the same
/// frame. Escape closes the dialog through [`Modal`] itself.
pub fn dialog_buttons(ui: &mut Ui, buttons: &[DialogButton<'_>]) -> Option<usize> {
    ui.add_space(12.0);
    let font = TextStyle::Button.resolve(ui.style());
    let padding = ui.spacing().button_padding.x;
    let spacing = ui.spacing().item_spacing.x;
    let width: f32 = buttons
        .iter()
        .map(|button| {
            let galley = ui
                .painter()
                .layout_no_wrap(button.text.to_owned(), font.clone(), Color32::PLACEHOLDER);
            galley.size().x + 2.0 * padding + spacing
        })
        .sum();
    let mut pressed = None;
    ui.horizontal(|ui| {
        ui.add_space((ui.available_width() - width + spacing).max(0.0));
        for (index, button) in buttons.iter().enumerate() {
            let widget = match button.kind {
                DialogButtonKind::Plain => Button::new(button.text),
                DialogButtonKind::Primary => filled_button(button.text.to_owned()),
                DialogButtonKind::Destructive => destructive_button(button.text),
            };
            let response = ui.add_enabled(button.refusal.is_none(), widget);
            let response = match button.refusal {
                Some(refusal) => response.on_disabled_hover_text(refusal),
                None => response,
            };
            if response.clicked() {
                pressed = Some(index);
            }
        }
    });
    let primary = buttons.len().checked_sub(1);
    let enter = dialog_enter(ui) && primary.is_some_and(|last| buttons[last].refusal.is_none());
    pressed.or(primary.filter(|_| enter))
}

/// Returns whether Enter was pressed on a frame after the one that opened the dialog of `ui`. The press that opened a
/// dialog from a menu or a focused button would otherwise confirm it at once.
fn dialog_enter(ui: &Ui) -> bool {
    let id = ui.unique_id().with("henad_dialog_pass");
    let pass = ui.ctx().cumulative_pass_nr();
    let shown_before = ui
        .data(|data| data.get_temp::<u64>(id))
        .is_some_and(|last| last + 1 >= pass);
    ui.data_mut(|data| data.insert_temp(id, pass));
    shown_before && ui.input(|input| input.key_pressed(Key::Enter))
}

/// Adds a selectable full-width row of a dialog's list, `name` with `description` weak under it, and returns its
/// response: a radio button named `name`, selected while `selected` holds.
///
/// The row takes the theme's hover fill under the pointer and the keyboard focus, and the focus outline inside its edge
/// while it holds the focus. A selected row takes the theme's selection fill with its name in the selection's text
/// colour after a check, as the current item of an overlay does.
pub fn choice_row(ui: &mut Ui, name: &str, description: Option<&str>, selected: bool) -> Response {
    let background = ui.painter().add(Shape::Noop);
    let weak = ui.visuals().weak_text_color();
    let name_color = if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };
    let words = ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        ui.add_space(MARK_COLUMN);
        ui.vertical(|ui| {
            ui.add_space(3.0);
            let name = ui.add(egui::Label::new(RichText::new(name).color(name_color)).selectable(false));
            if let Some(description) = description {
                let detail = RichText::new(description).color(weak).size(OVERLAY_DETAIL);
                ui.add(egui::Label::new(detail).wrap().selectable(false));
            }
            ui.add_space(3.0);
            name.rect
        })
        .inner
    });
    let name_rect = words.inner;
    let row = Rect::from_x_y_ranges(
        ui.max_rect().x_range().expand(OVERLAY_PADDING),
        words.response.rect.y_range(),
    );
    let response = ui.interact(row, ui.id().with(("henad_choice_row", name)), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, ui.is_enabled(), selected, name));
    if let Some(fill) = list_row_fill(ui.visuals(), selected, response.hovered() || response.has_focus()) {
        ui.painter()
            .set(background, Shape::rect_filled(row, CornerRadius::ZERO, fill));
    }
    paint_list_row_marks(ui, row, selected, response.has_focus(), name_rect.center().y);
    response
}

/// Returns the fill of a row of a list, an overlay's item or a dialog's choice, from `visuals`: the selection fill while
/// `chosen`, else the hover fill while `active`, under the pointer or the focus, and none otherwise.
fn list_row_fill(visuals: &egui::Visuals, chosen: bool, active: bool) -> Option<Color32> {
    if chosen {
        Some(visuals.selection.bg_fill)
    } else if active {
        Some(visuals.widgets.hovered.weak_bg_fill)
    } else {
        None
    }
}

/// Paints the marks of `row`, a row of a list: the focus outline inside it while `focused`, and a check in the
/// selection's text colour at its left, centred on `check_y`, while `chosen`.
fn paint_list_row_marks(ui: &Ui, row: Rect, chosen: bool, focused: bool, check_y: f32) {
    let visuals = ui.visuals();
    let painter = ui.painter();
    if focused {
        painter.rect_stroke(
            row,
            CornerRadius::ZERO,
            visuals.widgets.active.bg_stroke,
            StrokeKind::Inside,
        );
    }
    if chosen {
        painter.text(
            pos2(row.left() + OVERLAY_PADDING, check_y),
            egui::Align2::LEFT_CENTER,
            MDI_CHECK,
            FontId::proportional(SMALL_ICON),
            visuals.selection.stroke.color,
        );
    }
}

/// Width in points of a picker, the card's and the Sheet pane's alike. Its words wrap at it, and a picker with less to
/// show keeps it, so every picker opens as wide as the next.
pub const PICKER_WIDTH: f32 = 300.0;

/// Side in points of a colour swatch button.
const SWATCH_SIDE: f32 = 14.0;

/// Width in points of the New parameter dialog's content.
const DIALOG_WIDTH: f32 = 380.0;

/// Gives the accessible node `id` the name `name`, in place of the name its widget gave it.
pub fn set_name(ui: &Ui, id: Id, name: &str) {
    ui.ctx().accesskit_node_builder(id, |node| node.set_label(name));
}

/// Returns the reason `error` gives for a refused edit as a sentence. A reserved id reads as a name Henad keeps for
/// itself, and never names the id.
pub fn refusal_text(error: &SheetEditError) -> String {
    if let SheetEditError::Reserved(_) = error {
        return henad_graph::sheet::sentence::NAME_RESERVED.to_owned();
    }
    let text = error.to_string();
    if text.is_empty() {
        return text;
    }
    format!("{}.", capitalize(&text))
}

/// Returns `color`, the red, green, blue and alpha bytes of a sheet's colour, as egui draws it.
pub fn color32(color: Color) -> Color32 {
    let [red, green, blue, alpha] = color;
    Color32::from_rgba_unmultiplied(red, green, blue, alpha)
}

/// Colours of a chip: its fill, its name and its glyph or lead word.
struct ChipColors {
    /// Fill of the chip.
    fill: Color32,
    /// Colour of the chip's name.
    text: Color32,
    /// Colour of the lead word and of the slider mark.
    lead: Color32,
}

/// Returns the colours of a chip of `kind`: red for a result that its line cannot use, blue for a result, and neutral
/// otherwise.
fn chip_colors(kind: &ChipKind, red: bool) -> ChipColors {
    let (fill, text, lead) = match (kind, red) {
        (_, true) => (mcs::RED_900, mcs::RED_200, mcs::RED_200),
        (ChipKind::Result(_), false) => (mcs::BLUE_900, mcs::BLUE_200, mcs::BLUE_200),
        _ => (mcs::GRAY_800, mcs::GRAY_100, mcs::GRAY_400),
    };
    ChipColors { fill, text, lead }
}

/// Adds the chip of `chip`, a button named by its visible text, under the widget id `id` when one is given, so another
/// frame can open its picker or give it the focus, and returns its response.
///
/// A result chip is blue, or red and struck through when `red` holds, and every other chip neutral. A parameter chip
/// carries a slider mark, a state chip a swatch of the colour `swatch`, a property chip its lead word ("my"), "any
/// state" is in italics and a number in the mono face. Clicking a chip does nothing.
///
/// The chip is a button in egui's look with its own fill: the theme's outline and expansion under the pointer and on
/// focus, and while the picker of `id` is open the selection fill and text colour of a selected button. The eye then
/// finds what the picker edits. A red chip keeps its colours while its picker is open, inside the selection's outline.
pub fn chip_with_id(ui: &mut Ui, id: Option<Id>, chip: &SentenceChip, red: bool, swatch: Option<Color32>) -> Response {
    chip_within(ui, id, chip, red, swatch, f32::INFINITY)
}

/// Returns the width in points of the chip of `chip` with no swatch, as [`chip_within`] lays it out with room for its
/// whole name.
pub fn chip_width(ui: &Ui, chip: &SentenceChip) -> f32 {
    let body = TextStyle::Body.resolve(ui.style());
    let font = if chip.kind == ChipKind::Literal {
        FontId::monospace(body.size - 1.0)
    } else {
        body.clone()
    };
    let width = |text: &str, font: FontId| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
            .size()
            .x
    };
    let mark = if matches!(chip.kind, ChipKind::Parameter(_)) {
        width(MDI_TUNE_VERTICAL, body.clone()) + CHIP_GAP
    } else {
        0.0
    };
    let lead = if chip.lead.is_empty() {
        0.0
    } else {
        width(chip.lead, body) + CHIP_GAP
    };
    CHIP_PADDING * 2.0 + mark + lead + width(&chip.name, font)
}

/// Adds the chip of `chip` as [`chip_with_id`] does, at most `max_width` wide. A name too long for that width ends in
/// an ellipsis, and the chip's whole text shows as its tooltip.
pub fn chip_within(
    ui: &mut Ui,
    id: Option<Id>,
    chip: &SentenceChip,
    red: bool,
    swatch: Option<Color32>,
    max_width: f32,
) -> Response {
    let open = picker_open(ui, id);
    let colors = chip_colors(&chip.kind, red);
    let body = TextStyle::Body.resolve(ui.style());
    let font = if chip.kind == ChipKind::Literal {
        FontId::monospace(body.size - 1.0)
    } else {
        body.clone()
    };
    let painter = ui.painter();
    let mark = matches!(chip.kind, ChipKind::Parameter(_))
        .then(|| painter.layout_no_wrap(MDI_TUNE_VERTICAL.to_owned(), body.clone(), colors.lead));
    let lead = (!chip.lead.is_empty()).then(|| painter.layout_no_wrap(chip.lead.to_owned(), body.clone(), colors.lead));
    let swatch_width = if swatch.is_some() { CHIP_SWATCH + CHIP_GAP } else { 0.0 };
    let frame = CHIP_PADDING * 2.0
        + swatch_width
        + mark.as_ref().map_or(0.0, |galley| galley.size().x + CHIP_GAP)
        + lead.as_ref().map_or(0.0, |galley| galley.size().x + CHIP_GAP);
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &chip.name,
        0.0,
        egui::TextFormat {
            font_id: font,
            color: colors.text,
            italics: chip.kind == ChipKind::AnyState,
            ..Default::default()
        },
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((max_width - frame).max(0.0));
    let name = painter.layout_job(job);
    let elided = name.elided;
    let width = frame + name.size().x;
    let (rect, response) = allocate_control(ui, id, vec2(width, CHIP_HEIGHT));
    let response = if elided {
        response.on_hover_or_focus_text(&chip.text())
    } else {
        response
    };
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, chip.text()));
    if ui.is_rect_visible(rect) {
        // A red chip keeps its red while its picker is open. The hue tells that its line cannot use it.
        let selected = open && !red;
        let visuals = paint_button(ui, rect, &response, selected, Framed::Always, Some(colors.fill));
        if open && red {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::ZERO,
                ui.visuals().selection.stroke,
                StrokeKind::Inside,
            );
        }
        let (text_color, lead_color) = if selected {
            (visuals.text_color(), visuals.text_color())
        } else {
            (colors.text, colors.lead)
        };
        let painter = ui.painter();
        let mut x = rect.left() + CHIP_PADDING;
        let center_y = rect.center().y;
        if let Some(color) = swatch {
            self::swatch(painter, Pos2::new(x + CHIP_SWATCH / 2.0, center_y), color);
            x += swatch_width;
        }
        for galley in [mark, lead].into_iter().flatten() {
            let size = galley.size();
            painter.galley(Pos2::new(x, center_y - size.y / 2.0), galley, lead_color);
            x += size.x + CHIP_GAP;
        }
        let size = name.size();
        let name_rect = Rect::from_min_size(Pos2::new(x, center_y - size.y / 2.0), size);
        painter.galley(name_rect.min, name, text_color);
        if red {
            painter.hline(name_rect.x_range(), center_y, Stroke::new(1.0, text_color));
        }
    }
    response
}

/// Adds a blank showing `word`, named `name`, and returns its response with the index of its shapes: the word weak
/// inside a faded outline ([`slot_outline`]).
///
/// A blank with `painted` false is laid out and named but not painted, and returns the index of an empty shape that
/// the caller fills to paint it. The blank takes the widget id `id` when one is given. Under the pointer its outline
/// is brighter and its word takes the text colour, and while the picker of `id` is open it takes the selection fill
/// and text colour of a selected button.
pub fn blank_ui(
    ui: &mut Ui,
    id: Option<Id>,
    word: &str,
    name: &str,
    painted: bool,
) -> (Option<egui::layers::ShapeIdx>, Response) {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(word.to_owned(), font, mcs::GRAY_400);
    let (rect, response) = allocate_control(ui, id, vec2(galley.size().x + CHIP_PADDING * 2.0, CHIP_HEIGHT));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if !painted {
        return (Some(ui.painter().add(Shape::Noop)), response);
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if picker_open(ui, id) {
            let visuals = paint_button(ui, rect, &response, true, Framed::Always, None);
            ui.painter().add(blank_word(ui, rect, word, visuals.text_color()));
        } else if response.hovered() {
            painter.rect_stroke(rect, CornerRadius::ZERO, slot_outline(true), StrokeKind::Inside);
            painter.add(blank_word(ui, rect, word, ui.visuals().text_color()));
        } else {
            painter.extend(blank_shapes(ui, rect, word, false));
        }
        if response.has_focus() {
            focus_ring(ui, rect);
        }
    }
    (None, response)
}

/// Returns the outline of an empty slot (a blank, an empty select, an empty card) or of an add button: 1 point of gray
/// 800, or of gray 600 while `bright`, under the pointer or open.
pub fn slot_outline(bright: bool) -> Stroke {
    Stroke::new(1.0, if bright { mcs::GRAY_600 } else { mcs::GRAY_800 })
}

/// Returns the shape of the word `word` of a blank in `rect`, in `color`.
fn blank_word(ui: &Ui, rect: Rect, word: &str, color: Color32) -> Shape {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(word.to_owned(), font, color);
    let size = galley.size();
    Shape::galley(
        Pos2::new(rect.left() + CHIP_PADDING, rect.center().y - size.y / 2.0),
        galley,
        color,
    )
}

/// Allocates a clickable control of `size`, under the widget id `id` when one is given, and returns its rectangle and
/// response.
pub fn allocate_control(ui: &mut Ui, id: Option<Id>, size: Vec2) -> (Rect, Response) {
    match id {
        Some(id) => {
            let (_, rect) = ui.allocate_space(size);
            (rect, ui.interact(rect, id, Sense::click()))
        }
        None => ui.allocate_exact_size(size, Sense::click()),
    }
}

/// Returns the shapes of a blank showing `word` in `rect`: the word weak inside a faded outline ([`slot_outline`]), or
/// while `quiet` holds, as an optional blank is until its line is hovered, the weak word with no outline.
pub fn blank_shapes(ui: &Ui, rect: Rect, word: &str, quiet: bool) -> Vec<Shape> {
    let mut shapes = Vec::with_capacity(2);
    if !quiet {
        shapes.push(Shape::rect_stroke(
            rect,
            CornerRadius::ZERO,
            slot_outline(false),
            StrokeKind::Inside,
        ));
    }
    shapes.push(blank_word(ui, rect, word, ui.visuals().weak_text_color()));
    shapes
}

// The number field

/// Text typed in a number field and the message under it, kept while the field holds text it has not kept.
#[derive(Debug, Clone, Default)]
struct FieldDraft {
    /// Text in the field.
    text: String,
    /// Message under the field after the text was refused, `None` while none shows.
    message: Option<String>,
}

/// Returns the id the draft of the field `id` is kept under.
fn draft_id(id: Id) -> Id {
    id.with("henad_field_draft")
}

/// Adds a text field named `name` that shows `current` and commits what is typed on Enter or on losing focus.
///
/// `commit` reads the typed text and returns the message to show under the field, or `None` once the text is kept.
/// Text equal to `current` commits nothing while no message shows. A refused text stays in the field with its message,
/// which [`field_message`] returns. Typing alone changes nothing, and Escape puts `current` back and drops the
/// message.
pub fn commit_field(
    ui: &mut Ui,
    id: Id,
    name: &str,
    current: &str,
    width: f32,
    commit: impl FnOnce(&str) -> Option<String>,
) -> Response {
    commit_edit(
        ui,
        id,
        name,
        current,
        |text| TextEdit::singleline(text).desired_width(width),
        EscapeCancels(true),
        commit,
    )
}

/// Adds a number field as [`commit_field`] does, its text aligned to the right edge.
pub fn commit_number(
    ui: &mut Ui,
    id: Id,
    name: &str,
    current: &str,
    width: f32,
    commit: impl FnOnce(&str) -> Option<String>,
) -> Response {
    commit_edit(
        ui,
        id,
        name,
        current,
        |text| {
            TextEdit::singleline(text)
                .desired_width(width)
                .horizontal_align(egui::Align::RIGHT)
        },
        EscapeCancels(true),
        commit,
    )
}

/// Adds a field of several lines named `name` that shows `current`, with the grey `hint` while it is empty, and commits
/// what is typed on losing focus.
///
/// Enter starts a new line, and Escape commits as a click elsewhere does. `commit` takes the typed text as
/// [`commit_field`] does.
pub fn commit_area(
    ui: &mut Ui,
    id: Id,
    name: &str,
    current: &str,
    hint: &str,
    commit: impl FnOnce(&str) -> Option<String>,
) -> Response {
    commit_edit(
        ui,
        id,
        name,
        current,
        |text| {
            TextEdit::multiline(text)
                .hint_text(hint)
                .desired_rows(2)
                .desired_width(f32::INFINITY)
        },
        EscapeCancels(false),
        commit,
    )
}

/// Adds a field named `name` that shows `current`, with the grey `hint` while it is empty, and offers `suggestions`
/// under it while it holds the focus. A click on a suggestion commits it.
///
/// `commit` takes the typed text, or the suggestion chosen, as [`commit_field`] does.
#[expect(
    clippy::too_many_arguments,
    reason = "the field, its words and its suggestions each take a part"
)]
pub fn suggestion_field(
    ui: &mut Ui,
    id: Id,
    name: &str,
    current: &str,
    hint: &str,
    width: f32,
    suggestions: &[String],
    mut commit: impl FnMut(&str) -> Option<String>,
) -> Response {
    let response = commit_edit(
        ui,
        id,
        name,
        current,
        |text| TextEdit::singleline(text).hint_text(hint).desired_width(width),
        EscapeCancels(true),
        &mut commit,
    );
    let popup_id = id.with("henad_suggestions");
    let open_id = popup_id.with("open");
    let rect_id = popup_id.with("rect");
    let mut open = response.gained_focus() || ui.data(|data| data.get_temp(open_id)).unwrap_or(false);
    // Enter, Tab or Escape closes the suggestions. A press closes them unless it lands on the field or on them.
    let pointer_used =
        ui.input(|input| input.pointer.any_pressed() || input.pointer.any_down() || input.pointer.any_released());
    if response.lost_focus() && !pointer_used {
        open = false;
    }
    let pressed = ui.input(|input| {
        input
            .pointer
            .any_pressed()
            .then(|| input.pointer.interact_pos())
            .flatten()
    });
    if let Some(position) = pressed {
        let shown: Option<Rect> = ui.data(|data| data.get_temp(rect_id));
        if !response.rect.contains(position) && !shown.is_some_and(|rect| rect.contains(position)) {
            open = false;
        }
    }
    let popup = Popup::from_response(&response)
        .id(popup_id)
        .open(open)
        .close_behavior(PopupCloseBehavior::IgnoreClicks)
        .show(|ui| {
            let mut chosen = None;
            for suggestion in suggestions {
                if ui.add(Button::new(suggestion).frame(false)).clicked() {
                    chosen = Some(suggestion.clone());
                }
            }
            chosen
        });
    let chosen = popup.as_ref().and_then(|popup| popup.inner.clone());
    if let Some(popup) = &popup {
        let rect = popup.response.rect;
        ui.data_mut(|data| data.insert_temp(rect_id, rect));
    }
    if chosen.is_some() {
        open = false;
    }
    ui.data_mut(|data| data.insert_temp(open_id, open));
    if let Some(suggestion) = chosen {
        forget_field(ui, id);
        if let Some(message) = commit(&suggestion) {
            ui.data_mut(|data| {
                data.insert_temp(
                    draft_id(id),
                    FieldDraft {
                        text: suggestion,
                        message: Some(message),
                    },
                );
            });
        }
    }
    response
}

/// Whether Escape in a field puts back what the sheet holds. Otherwise Escape commits the typed text.
#[derive(Debug, Clone, Copy)]
struct EscapeCancels(bool);

/// Adds the text field that `edit` makes over the text, named `name`, showing `current`, and commits what is typed on
/// losing focus through `commit`, as [`commit_field`] does. Escape cancels when `escape` holds.
fn commit_edit(
    ui: &mut Ui,
    id: Id,
    name: &str,
    current: &str,
    edit: impl for<'t> FnOnce(&'t mut String) -> TextEdit<'t>,
    escape: EscapeCancels,
    commit: impl FnOnce(&str) -> Option<String>,
) -> Response {
    let draft: Option<FieldDraft> = ui.data(|data| data.get_temp(draft_id(id)));
    let mut text = draft
        .as_ref()
        .map_or_else(|| current.to_owned(), |draft| draft.text.clone());
    let response = ui.add(edit(&mut text).id(id));
    set_name(ui, response.id, name);
    let message = draft.and_then(|draft| draft.message);
    let canceled = escape.0 && response.lost_focus() && ui.input(|input| input.key_pressed(Key::Escape));
    if canceled {
        forget_field(ui, id);
    } else if response.lost_focus() {
        let refused = if message.is_none() && text.trim() == current.trim() {
            None
        } else {
            commit(&text)
        };
        ui.data_mut(|data| match refused {
            Some(message) => {
                data.insert_temp(
                    draft_id(id),
                    FieldDraft {
                        text,
                        message: Some(message),
                    },
                );
            }
            None => {
                data.remove::<FieldDraft>(draft_id(id));
            }
        });
    } else if ui.memory(|memory| memory.has_focus(response.id)) {
        // `Response::has_focus` also asks that the window holds the system's focus, and the text edit takes typed text
        // without it.
        ui.data_mut(|data| data.insert_temp(draft_id(id), FieldDraft { text, message }));
    }
    response
}

/// Returns the message under the field `id` after its text was refused, `None` while none shows.
pub fn field_message(ui: &Ui, id: Id) -> Option<String> {
    let draft = ui.data(|data| data.get_temp::<FieldDraft>(draft_id(id)))?;
    draft.message
}

/// Drops the refused text and the message of the field `id`, so it shows the value it commits again.
pub fn forget_field(ui: &Ui, id: Id) {
    ui.data_mut(|data| data.remove::<FieldDraft>(draft_id(id)));
}

/// Returns whether one of `names` is `name`, case aside.
pub fn name_taken<'a>(name: &str, names: impl IntoIterator<Item = &'a str>) -> bool {
    let name = name.to_lowercase();
    names.into_iter().any(|known| known.to_lowercase() == name)
}

/// Draws `message` in the theme's error colour under a row of fields.
pub fn message_ui(ui: &mut Ui, message: &str) {
    ui.colored_label(ui.visuals().error_fg_color, message);
}

// The Number box

/// Range a typed number is read in: a chance, a rate, a share kept or any number. A picker's Number box and the New
/// parameter dialog accept numbers in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberUse {
    /// A chance, from 0 to 1.
    Chance,
    /// A rate, from 0 up.
    Rate,
    /// Any number.
    Number,
    /// A number of times, an integer from 0.
    Times,
    /// The share of a mark kept each tick, from 0 to 1.
    Share,
}

impl NumberUse {
    /// Returns the number typed as `text`, or the message under a text that does not read or lies outside the range.
    ///
    /// # Errors
    ///
    /// Returns the message for text that is not one number, a chance or a share outside 0 to 1, or a rate below 0.
    pub fn read(self, text: &str) -> Result<SheetLiteral, &'static str> {
        let literal = read_typed_number(text).ok_or(TYPE_ONE_NUMBER)?;
        let number = literal.number().unwrap_or_default();
        match self {
            Self::Chance if !(0.0..=1.0).contains(&number) => Err(CHANCE_OUT_OF_RANGE),
            Self::Share if !(0.0..=1.0).contains(&number) => Err(KEPT_OUT_OF_RANGE),
            Self::Rate if number < 0.0 => Err(RATE_BELOW_ZERO),
            Self::Times if number < 0.0 || number.fract() != 0.0 => Err(TYPE_TIMES),
            _ => Ok(literal),
        }
    }

    /// Returns the hint over a Number box that reads in this range. It also names the box.
    pub fn box_hint(self) -> &'static str {
        match self {
            Self::Chance => CHANCE_BOX_HINT,
            Self::Share => SHARE_BOX_HINT,
            Self::Rate | Self::Number | Self::Times => NUMBER_BOX_HINT,
        }
    }
}

// The colour palette

/// Returns the name of the palette colour nearest `color`, such as "Dark grey".
pub fn color_name(color: Color) -> &'static str {
    let distance = |palette: &Color| -> u32 {
        palette
            .iter()
            .zip(color)
            .take(3)
            .map(|(a, b)| u32::from(a.abs_diff(b)).pow(2))
            .sum()
    };
    PALETTE_COLORS
        .iter()
        .zip(COLOR_NAMES)
        .min_by_key(|(palette, _)| distance(palette))
        .map_or(COLOR_NAMES[0], |(_, name)| name)
}

/// Adds a swatch of `color`, a button named `name` that opens "Choose a colour", and returns the colour chosen.
///
/// "Choose a colour" is an overlay of the twelve palette colours, six to a row.
pub fn color_button(ui: &mut Ui, name: &str, color: Color) -> Option<Color> {
    let id = ui.next_auto_id();
    let popup_id = id.with("picker");
    let was_open = Popup::is_id_open(ui.ctx(), popup_id);
    let response = swatch_button(ui, color32(color), name, was_open);
    let (popup, _) = placed(
        ui.ctx(),
        Popup::from_toggle_button_response(&response)
            .id(popup_id)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .frame(Frame::NONE),
    );
    let popup = popup.show(|ui| {
        overlay(ui, |ui| {
            overlay_title(ui, CHOOSE_A_COLOR);
            ui.add_space(6.0);
            let mut chosen = None;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                ui.set_max_width(6.0 * (SWATCH_SIDE + 8.0));
                for (palette, name) in PALETTE_COLORS.iter().zip(COLOR_NAMES) {
                    if swatch_button(ui, color32(*palette), name, false)
                        .on_hover_text(name)
                        .clicked()
                    {
                        chosen = Some(*palette);
                    }
                }
            });
            ui.add_space(2.0);
            if chosen.is_some() {
                ui.close();
            }
            (chosen, ui.unique_id())
        })
    });
    escape_to_anchor(ui, &response, popup_id, was_open);
    let popup = popup?;
    let (chosen, id) = popup.inner;
    accessible_group(
        ui.ctx(),
        id,
        egui::accesskit::Role::Dialog,
        CHOOSE_A_COLOR,
        popup.response.rect,
    );
    chosen
}

/// Adds a square swatch of `color`, a button named `name`, and returns its response.
///
/// The outline is gray 700, the theme's hover outline under the pointer and the selection's outline while the picker it
/// opens is `open`. Keyboard focus draws the focus outline.
fn swatch_button(ui: &mut Ui, color: Color32, name: &str, open: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(SWATCH_SIDE + 4.0, CHIP_HEIGHT), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let square = Rect::from_center_size(rect.center(), Vec2::splat(SWATCH_SIDE));
        let painter = ui.painter();
        painter.rect_filled(square, CornerRadius::ZERO, color);
        let outline = if open {
            ui.visuals().selection.stroke
        } else if response.hovered() {
            ui.visuals().widgets.hovered.bg_stroke
        } else {
            Stroke::new(1.0, mcs::GRAY_700)
        };
        painter.rect_stroke(square, CornerRadius::ZERO, outline, egui::StrokeKind::Inside);
        if response.has_focus() {
            focus_ring(ui, square);
        }
    }
    response
}

// The New parameter dialog

/// State of the New parameter dialog while it is open: what the parameter is for and the text of each field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewParameterDialog {
    /// Range the new parameter's values are read in.
    pub number_use: NumberUse,
    /// Text of the Name field.
    pub name: String,
    /// Text of the Default field.
    pub default: String,
    /// Text of the Min field.
    pub min: String,
    /// Text of the Max field.
    pub max: String,
    /// Text of the Unit field.
    pub unit: String,
    /// Whether the author typed in Min. Until then a negative default sets Min to twice it.
    min_typed: bool,
    /// Whether the author typed in Max. Until then Max follows Default.
    max_typed: bool,
    /// Message of the last refused Create, `None` while none shows.
    pub message: Option<String>,
    /// Whether the Name field takes the focus on the next frame.
    focus_name: bool,
}

/// A parameter the New parameter dialog creates.
#[derive(Debug, Clone, PartialEq)]
pub struct NewParameter {
    /// Name of the parameter.
    pub name: String,
    /// Value the model keeps.
    pub default: f64,
    /// Smallest value of the slider.
    pub min: f64,
    /// Largest value of the slider.
    pub max: f64,
    /// Unit of the value, `None` for none.
    pub unit: Option<String>,
    /// Type of the parameter: an integer for a number of times, a number otherwise.
    pub parameter_type: ParameterType,
}

impl NewParameter {
    /// Returns the command that adds the parameter to a sheet.
    pub fn add_edit(self) -> SheetEdit {
        SheetEdit::AddParameter {
            name: self.name,
            parameter_type: self.parameter_type,
            min: self.min,
            max: self.max,
            default: self.default,
            unit: self.unit,
        }
    }
}

/// Returns the names of the parameters of `document`, which the New parameter dialog refuses for a new one.
pub fn parameter_names(document: &SheetDocument) -> Vec<&str> {
    document
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect()
}

/// Outcome of a frame of the New parameter dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum DialogOutcome {
    /// The dialog stays open.
    Open,
    /// Cancel, Escape or a click beside the dialog closed it.
    Canceled,
    /// Create parameter made this parameter.
    Created(NewParameter),
}

impl NewParameterDialog {
    /// Returns the dialog for a parameter that stands for `number_use`, from the number `typed` in the field or blank
    /// it is for, with the unit `unit`.
    ///
    /// A chance or a rate starts from 0.1 in 0 to 1, a share kept from 1 in 0 to 1, any other number from 1 in 0 to 10,
    /// and a typed number from itself with a Max about ten times it.
    pub fn new(number_use: NumberUse, typed: Option<f64>, unit: Option<String>) -> Self {
        let (mut default, mut min, mut max) = match number_use {
            NumberUse::Chance | NumberUse::Rate => (0.1, 0.0, 1.0),
            NumberUse::Share => (1.0, 0.0, 1.0),
            NumberUse::Number | NumberUse::Times => (1.0, 0.0, 10.0),
        };
        if let Some(value) = typed.filter(|value| value.is_finite()) {
            default = value;
            if value < 0.0 {
                min = value * 2.0;
            }
            max = default_max(value, number_use);
        }
        let text = |value: f64| format_number(value).unwrap_or_default();
        Self {
            number_use,
            name: String::new(),
            default: text(default),
            min: text(min),
            max: text(max),
            unit: unit.unwrap_or_default(),
            min_typed: false,
            max_typed: false,
            message: None,
            focus_name: true,
        }
    }

    /// Sets the text of the Default field to `text`. Min and Max follow it until the author types in them.
    pub fn set_default(&mut self, text: String) {
        self.default = text;
        let Some(value) = read_typed_number(&self.default).and_then(SheetLiteral::number) else {
            return;
        };
        if !self.max_typed {
            self.max = format_number(default_max(value, self.number_use)).unwrap_or_default();
        }
        if !self.min_typed && value < 0.0 {
            self.min = format_number(value * 2.0).unwrap_or_default();
        }
    }

    /// Returns the parameter the fields describe, or the message for the first field that is wrong. `taken` holds
    /// the names of the sheet's parameters.
    ///
    /// # Errors
    ///
    /// Returns the message on an empty or taken name, a field that does not read as a number, Min not below Max, a
    /// default outside Min and Max, or a chance's or a share's range outside 0 to 1.
    pub fn parameter(&self, taken: &[&str]) -> Result<NewParameter, String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(DIALOG_NAME_MISSING.to_owned());
        }
        if name_taken(name, taken.iter().copied()) {
            return Err(parameter_name_taken(name));
        }
        if is_reserved_name(name, IdSpace::Item) {
            return Err(henad_graph::sheet::sentence::name_reserved(name));
        }
        let read = |text: &str| read_typed_number(text).and_then(SheetLiteral::number);
        let (Some(default), Some(min), Some(max)) = (read(&self.default), read(&self.min), read(&self.max)) else {
            return Err(LIMITS_NOT_NUMBERS.to_owned());
        };
        if min >= max {
            return Err(MIN_NOT_BELOW_MAX.to_owned());
        }
        if default < min || default > max {
            return Err(DEFAULT_OUTSIDE_LIMITS.to_owned());
        }
        if self.number_use == NumberUse::Chance && (min < 0.0 || max > 1.0) {
            return Err(CHANCE_LIMITS_OUTSIDE.to_owned());
        }
        if self.number_use == NumberUse::Share && (min < 0.0 || max > 1.0) {
            return Err(SHARE_LIMITS_OUTSIDE.to_owned());
        }
        let times = self.number_use == NumberUse::Times;
        if times
            && [default, min, max]
                .iter()
                .any(|value| *value < 0.0 || value.fract() != 0.0)
        {
            return Err(TIMES_LIMITS_NOT_INTEGERS.to_owned());
        }
        let unit = self.unit.trim();
        Ok(NewParameter {
            name: name.to_owned(),
            default,
            min,
            max,
            unit: (!unit.is_empty()).then(|| unit.to_owned()),
            parameter_type: if times {
                ParameterType::WholeNumber
            } else {
                ParameterType::Number
            },
        })
    }
}

/// Returns the default top of a parameter's range for `value`: a round number about ten times it, at most 1 for a
/// value below 1, a chance or a share.
///
/// The round numbers are 1, 2 and 5 times a power of ten. A value of 0 takes 10 for a number and 1 otherwise.
pub fn default_max(value: f64, number_use: NumberUse) -> f64 {
    let size = value.abs();
    if size.is_nan() || size == 0.0 {
        return if matches!(number_use, NumberUse::Number | NumberUse::Times) {
            10.0
        } else {
            1.0
        };
    }
    let target = 10.0 * size;
    let power = 10_f64.powf(target.log10().floor());
    let round = [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|step| step * power)
        .find(|candidate| *candidate >= target - 1e-12)
        .unwrap_or(10.0 * power);
    // Three significant figures drop the float noise of the power of ten.
    let top = format!("{round:.2e}").parse().unwrap_or(round);
    if size < 1.0 || matches!(number_use, NumberUse::Chance | NumberUse::Share) {
        top.min(1.0)
    } else {
        top
    }
}

/// Draws the New parameter dialog over `dialog` for one frame, and returns its outcome. `taken` holds the names of the
/// sheet's parameters.
pub fn new_parameter_modal(ctx: &egui::Context, dialog: &mut NewParameterDialog, taken: &[&str]) -> DialogOutcome {
    let mut create = false;
    let mut canceled = false;
    let modal = Modal::new(Id::new("henad_new_parameter_modal")).frame(dialog_frame(ctx));
    let response = modal.show(ctx, |ui| {
        dialog_title(ui, NEW_PARAMETER_HEADING, NEW_PARAMETER_HEADING, DIALOG_WIDTH);
        ui.label(NEW_PARAMETER_LEAD);
        ui.add_space(8.0);
        let placeholder = match dialog.number_use {
            NumberUse::Chance => CHANCE_PARAMETER_PLACEHOLDER,
            NumberUse::Rate => RATE_PARAMETER_PLACEHOLDER,
            NumberUse::Number => NUMBER_PARAMETER_PLACEHOLDER,
            NumberUse::Times => TIMES_PARAMETER_PLACEHOLDER,
            NumberUse::Share => SHARE_PARAMETER_PLACEHOLDER,
        };
        egui::Grid::new("henad_new_parameter_fields")
            .num_columns(2)
            .spacing(vec2(12.0, 4.0))
            .show(ui, |ui| {
                ui.label(NAME_LABEL);
                let name = ui.add(TextEdit::singleline(&mut dialog.name).hint_text(placeholder));
                set_name(ui, name.id, PARAMETER_NAME_FIELD);
                if std::mem::take(&mut dialog.focus_name) {
                    name.request_focus();
                }
                ui.end_row();
                ui.label(DEFAULT_LABEL);
                let mut default = dialog.default.clone();
                let field = ui.add(TextEdit::singleline(&mut default));
                set_name(ui, field.id, DEFAULT_NAME);
                if field.changed() {
                    dialog.set_default(default);
                }
                ui.end_row();
                ui.label(MIN_LABEL);
                let field = ui.add(TextEdit::singleline(&mut dialog.min));
                set_name(ui, field.id, MIN_LABEL);
                dialog.min_typed |= field.changed();
                ui.end_row();
                ui.label(MAX_LABEL);
                let field = ui.add(TextEdit::singleline(&mut dialog.max));
                set_name(ui, field.id, MAX_LABEL);
                dialog.max_typed |= field.changed();
                ui.end_row();
                ui.label(UNIT_LABEL);
                let field = ui.add(TextEdit::singleline(&mut dialog.unit).hint_text(DIALOG_UNIT_PLACEHOLDER));
                set_name(ui, field.id, UNIT_LABEL);
                ui.end_row();
            });
        if let Some(message) = &dialog.message {
            message_ui(ui, message);
        }
        let buttons = [
            DialogButton::new(CANCEL, DialogButtonKind::Plain),
            DialogButton::new(CREATE_PARAMETER, DialogButtonKind::Primary),
        ];
        match dialog_buttons(ui, &buttons) {
            Some(0) => canceled = true,
            Some(_) => create = true,
            None => {}
        }
    });
    if create {
        match dialog.parameter(taken) {
            Ok(parameter) => return DialogOutcome::Created(parameter),
            Err(message) => dialog.message = Some(message),
        }
    }
    if canceled || response.should_close() {
        return DialogOutcome::Canceled;
    }
    DialogOutcome::Open
}
