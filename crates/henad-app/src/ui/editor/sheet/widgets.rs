//! Controls the Sheet pane and the Card pane share: the chip, the dashed blank, a number field that commits on Enter
//! or on losing focus, the picker, the colour palette and the New parameter dialog. The Editor's notices and the parts
//! of its dialogs live here too.
//!
//! A *picker* is a popover under its anchor that lists what a blank or a field can take, in groups under headings, with
//! "New parameter..." and a box for a typed number. A group with no item is left out, and a picker with nothing to
//! offer says what is missing. A typed number takes a trailing "%" as a percent ("30%" is 0.3).

use egui::{
    Button, Color32, CornerRadius, FontId, Frame, Id, Key, Margin, Modal, Painter, Popup, PopupCloseBehavior, Pos2,
    Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, TextStyle, Ui, Vec2, WidgetInfo, WidgetType,
    pos2, vec2,
};
use henad_graph::sheet::document::{ParameterType, SheetDocument, SheetLiteral};
use henad_graph::sheet::edit::{IdSpace, PALETTE_COLORS, SheetEdit, SheetEditError, is_reserved_name};
use henad_graph::sheet::format_number;
use henad_graph::sheet::sentence::{
    AddButton, CANCEL, CHANCE_BOX_HINT, CHANCE_LIMITS_OUTSIDE, CHANCE_OUT_OF_RANGE, CHANCE_PARAMETER_PLACEHOLDER,
    CHOOSE_A_COLOR, COLOR_NAMES, CREATE_PARAMETER, ChipKind, DEFAULT_LABEL, DEFAULT_NAME, DEFAULT_OUTSIDE_LIMITS,
    DIALOG_NAME_MISSING, DIALOG_UNIT_PLACEHOLDER, LIMITS_NOT_NUMBERS, MAX_LABEL, MIN_LABEL, MIN_NOT_BELOW_MAX,
    NAME_LABEL, NEW_PARAMETER_HEADING, NEW_PARAMETER_LEAD, NUMBER_BOX_HINT, NUMBER_PARAMETER_PLACEHOLDER,
    PARAMETER_NAME_FIELD, RATE_BELOW_ZERO, RATE_PARAMETER_PLACEHOLDER, SentenceChip, TIMES_LIMITS_NOT_INTEGERS,
    TIMES_PARAMETER_PLACEHOLDER, TYPE_ONE_NUMBER, TYPE_TIMES, UNIT_LABEL, UNSAVED_MARK, UNSAVED_MARK_TOOLTIP,
    parameter_name_taken, read_typed_number,
};
use henad_graph::value::Color;

use super::accessible_group;
use crate::icons::material_design_icons::{
    MDI_ALERT, MDI_ALERT_CIRCLE, MDI_CHECK, MDI_CHEVRON_DOWN, MDI_CIRCLE_MEDIUM, MDI_DOTS_HORIZONTAL, MDI_INFORMATION,
    MDI_TUNE_VERTICAL,
};
use crate::ui::mcs;
use crate::ui::sweep::layout::{destructive_button, filled_button};
use crate::ui::text::capitalize;

/// Padding in points at each side of the words of a chip, a blank or a select.
pub const CHIP_PADDING: f32 = 5.0;

/// Height in points of a chip, a blank or a select.
pub const CHIP_HEIGHT: f32 = 20.0;

/// Gap in points between a chip's glyph, swatch or lead word and its name.
const CHIP_GAP: f32 = 4.0;

/// Side in points of the swatch of a state chip.
pub const CHIP_SWATCH: f32 = 9.0;

/// Width in points of the keyboard focus outline.
const FOCUS_WIDTH: f32 = 1.5;

/// Padding in points inside an overlay, and at the sides of its items.
pub const OVERLAY_PADDING: f32 = 6.0;

/// Height in points of an item of an overlay with no grey words under it.
const OVERLAY_ROW: f32 = 22.0;

/// Width in points of the column at the left of an overlay's items that holds the check of the current value.
const MARK_COLUMN: f32 = 16.0;

/// Size in points of the small mono labels of the editor: group labels, line numbers and counts.
pub const SMALL_LABEL: f32 = 11.0;

/// Size in points of the grey words under an item of an overlay.
const OVERLAY_DETAIL: f32 = 11.5;

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
// Each control the sheet editor paints itself is drawn by one function here, and no call site passes a colour. The
// colours follow the palette in `agents/ui.md`: a tinted fill is a hue's 900 with its 200 for text, a line or an
// outline in a hue is its 500, a neutral chip is gray 800 with gray 100 text, a neutral outline gray 700, an empty slot
// gray 500 dashes around a gray 400 word, a hairline gray 800, and an overlay the window fill inside gray 600.

/// Space in points between two framed regions.
pub const REGION_GAP: f32 = 12.0;

/// Draws a framed region, gray 900 inside a 1 point gray 800 border, with `add` inside it across the width it is
/// given, and returns what `add` returns. A card and each section of the Sheet pane is one.
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

/// Draws the title row of a region: `title` as a small upper-case mono label, then a hairline running to what `end`
/// draws at the right edge, such as the region's help.
pub fn region_title(ui: &mut Ui, title: &str, end: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        caption(ui, title);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            end(ui);
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter()
                .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, mcs::GRAY_800));
        });
    });
}

/// Paints the keyboard focus outline of a control whose border is `rect`: 1.5 points of blue 500 outside the border.
pub fn focus_ring(ui: &Ui, rect: Rect) {
    ui.painter().rect_stroke(
        rect,
        CornerRadius::ZERO,
        Stroke::new(FOCUS_WIDTH, mcs::BLUE_500),
        StrokeKind::Outside,
    );
}

/// Returns `text` as a small mono label in gray 400, for metadata such as a line number or a fire count.
pub fn small_label(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .font(FontId::monospace(SMALL_LABEL))
        .color(mcs::GRAY_400)
}

/// Adds `text` as a small upper-case mono label in gray 400, for a title or a count, and returns its response. The
/// label keeps `text` as written for its accessible value, as a label of `text` would hold.
pub fn caption(ui: &mut Ui, text: &str) -> Response {
    let label = ui.add(egui::Label::new(small_label(text.to_uppercase())).selectable(false));
    ui.ctx().accesskit_node_builder(label.id, |node| node.set_value(text));
    label
}

/// Draws `add` inside the frame of an overlay, a menu or a picker: the theme's window fill inside a 1 point gray 600
/// border, with no shadow, and returns what `add` returns.
///
/// egui fades an area in over its first frames. The overlay sets the opacity back to whole, so the text under it never
/// shows through. The overlay's popup takes [`Frame::NONE`].
pub fn overlay<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.set_opacity(1.0);
    Frame::NONE
        .fill(ui.visuals().window_fill)
        .stroke(Stroke::new(1.0, mcs::GRAY_600))
        .inner_margin(Margin::same(OVERLAY_PADDING as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

/// Draws the title of an overlay as a small upper-case mono label, with a hairline under it across the overlay.
pub fn overlay_title(ui: &mut Ui, title: &str) {
    ui.add_space(2.0);
    caption(ui, title);
    ui.add_space(4.0);
    overlay_rule(ui);
}

/// Draws the label of a group of an overlay's items as a small upper-case mono label, with a hairline above it across
/// the overlay.
pub fn overlay_group(ui: &mut Ui, label: &str) {
    ui.add_space(4.0);
    overlay_rule(ui);
    ui.add_space(4.0);
    caption(ui, label);
    ui.add_space(2.0);
}

/// Draws a hairline across an overlay, through its padding at both sides.
fn overlay_rule(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(1.0), 1.0), Sense::hover());
    let across = ui.max_rect().union(rect).x_range().expand(OVERLAY_PADDING);
    ui.painter()
        .hline(across, rect.center().y, Stroke::new(1.0, mcs::GRAY_800));
}

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
}

/// Adds `item` as a full-width row of an overlay and returns its response.
///
/// The row takes the gray 800 fill and the theme's hover text under the pointer and under keyboard focus, and the focus
/// outline inside its edge while it holds the focus. A current value fills blue 900 with its words in blue 200 after a check.
/// Inside a disabled `Ui` the words are weak and the row takes no hover and no click.
pub fn overlay_item(ui: &mut Ui, item: &OverlayItem<'_>) -> Response {
    let enabled = ui.is_enabled();
    let body = TextStyle::Body.resolve(ui.style());
    let swatch_width = item.swatch.map_or(0.0, |_| CHIP_SWATCH + CHIP_GAP);
    let indent = MARK_COLUMN + swatch_width;
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
            mcs::BLUE_200
        } else if active {
            visuals.widgets.hovered.text_color()
        } else {
            visuals.widgets.inactive.text_color()
        };
        let painter = ui.painter();
        if let Some(fill) = list_row_fill(enabled && item.current, active) {
            painter.rect_filled(row, CornerRadius::ZERO, fill);
        }
        let top = row.top() + (height - content) / 2.0;
        let line_center = top + label_size.y / 2.0;
        paint_list_row_marks(painter, row, item.current, response.has_focus(), line_center);
        let mut x = rect.left() + MARK_COLUMN;
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

/// Returns the width in points of a text control showing `text`, as [`text_control`] adds it.
pub fn text_control_width(ui: &Ui, text: &str) -> f32 {
    let font = TextStyle::Body.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
        .size()
        .x
        + 6.0
}

/// Adds a text control showing `text` in the weak text colour, named `name`, under the widget id `id` when one is
/// given, and returns its response: "always", "for each" and the "+" after a list.
///
/// Its hit area is as high as the row of the sentence `row`. A gray 700 outline shows under the pointer and while
/// its picker is open, and keyboard focus draws the focus outline. It is laid out as one unbroken piece, so a row that
/// has no room for it moves it to the next row whole.
pub fn text_control(ui: &mut Ui, id: Option<Id>, text: &str, name: &str, row: f32) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let color = ui.visuals().weak_text_color();
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let width = galley.size().x + 6.0;
    let (rect, response) = allocate_control(ui, id, vec2(width, row));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let boxed = Rect::from_center_size(rect.center(), vec2(width, CHIP_HEIGHT));
        let painter = ui.painter();
        if response.hovered() || picker_open(ui, id) {
            painter.rect_stroke(
                boxed,
                CornerRadius::ZERO,
                Stroke::new(1.0, mcs::GRAY_700),
                StrokeKind::Inside,
            );
        }
        painter.galley(
            Pos2::new(boxed.left() + 3.0, boxed.center().y - galley.size().y / 2.0),
            galley,
            color,
        );
        if response.has_focus() {
            focus_ring(ui, boxed);
        }
    }
    response
}

/// Adds an inline select showing `words` and a chevron, named `name`, under the widget id `id` when one is given, and
/// returns its response.
///
/// A filled select has a gray 700 outline, gray 400 under the pointer and blue 500 while its options are open. An
/// `empty` select is dashed gray 500 around its words in gray 400, as a blank is.
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
    let color = if !enabled {
        weak
    } else if empty {
        mcs::GRAY_400
    } else {
        mcs::GRAY_100
    };
    let painter = ui.painter();
    let chevron = painter.layout_no_wrap(MDI_CHEVRON_DOWN.to_owned(), FontId::proportional(SMALL_LABEL), weak);
    let frame = CHIP_PADDING + CHIP_GAP + chevron.size().x + 3.0;
    let mut job = egui::text::LayoutJob::simple_singleline(words.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width((max_width - frame).max(0.0));
    let text = painter.layout_job(job);
    let width = frame + text.size().x;
    let (rect, response) = allocate_control(ui, id, vec2(width, CHIP_HEIGHT));
    let response = if text.elided {
        response.on_hover_text(words)
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
        let outline = if open {
            Some(mcs::BLUE_500)
        } else if response.hovered() {
            Some(mcs::GRAY_400)
        } else {
            (!empty).then_some(mcs::GRAY_700)
        };
        let painter = ui.painter();
        match outline {
            Some(outline) => {
                painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, outline), StrokeKind::Inside);
            }
            None => painter.extend(dashed_rect(rect, Stroke::new(1.0, mcs::GRAY_500))),
        }
        let left = Pos2::new(rect.left() + CHIP_PADDING, rect.center().y - text.size().y / 2.0);
        let chevron_left = left.x + text.size().x + CHIP_GAP;
        painter.galley(left, text, color);
        painter.galley(
            Pos2::new(chevron_left, rect.center().y - chevron.size().y / 2.0),
            chevron,
            weak,
        );
        if response.has_focus() {
            focus_ring(ui, rect);
        }
    }
    response
}

/// Draws the list of options of a select under `anchor` while the popup `popup_id` is open, one row of `items` per
/// option, and returns the index of the option chosen.
///
/// A click on `anchor` opens and closes the list, and a choice or a click outside closes it. The list is an overlay.
pub fn select_list(ui: &Ui, anchor: &Response, popup_id: Id, items: &[OverlayItem<'_>]) -> Option<usize> {
    if anchor.clicked() {
        Popup::toggle_id(ui.ctx(), popup_id);
    }
    let popup = Popup::from_response(anchor)
        .id(popup_id)
        .open_memory(None)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .frame(Frame::NONE)
        .show(|ui| {
            overlay(ui, |ui| {
                let mut chosen = None;
                for (index, item) in items.iter().enumerate() {
                    if overlay_item(ui, item).clicked() {
                        chosen = Some(index);
                    }
                }
                if chosen.is_some() {
                    ui.close();
                }
                chosen
            })
        })?;
    popup.inner
}

/// Adds the chevron box after a condition word, named `name` under the widget id `id`, and returns its response.
///
/// The box is 14 points square, close after the word. Its hit area is as high as the row of the sentence `row`. The
/// outline is gray 700, gray 400 under the pointer and blue 500 while "Choose when" is open.
pub fn arrow_control(ui: &mut Ui, id: Id, name: &str, row: f32) -> Response {
    let (rect, response) = allocate_control(ui, Some(id), vec2(ARROW_WIDTH, row));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let boxed = Rect::from_center_size(
            pos2(rect.left() + 3.0 + ARROW_SIDE / 2.0, rect.center().y),
            Vec2::splat(ARROW_SIDE),
        );
        let outline = if picker_open(ui, Some(id)) {
            mcs::BLUE_500
        } else if response.hovered() {
            mcs::GRAY_400
        } else {
            mcs::GRAY_700
        };
        let painter = ui.painter();
        painter.rect_stroke(boxed, CornerRadius::ZERO, Stroke::new(1.0, outline), StrokeKind::Inside);
        painter.text(
            boxed.center(),
            egui::Align2::CENTER_CENTER,
            MDI_CHEVRON_DOWN,
            FontId::proportional(SMALL_LABEL),
            ui.visuals().text_color(),
        );
        if response.has_focus() {
            focus_ring(ui, boxed);
        }
    }
    response
}

/// Adds the menu button of a line, a dots glyph in the weak text colour, as large as `rect`, named `name` under the
/// widget id `id`, and returns its response. The menu is the popup `popup_id`.
///
/// The button takes the theme's hover fill under the pointer and the tinted fill while its menu is open.
pub fn menu_button(ui: &Ui, id: Id, popup_id: Id, name: &str, rect: Rect) -> Response {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let open = Popup::is_id_open(ui.ctx(), popup_id);
        let painter = ui.painter();
        let (fill, color) = if open {
            (Some(mcs::BLUE_900), mcs::BLUE_200)
        } else if response.hovered() {
            (
                Some(ui.visuals().widgets.hovered.weak_bg_fill),
                ui.visuals().strong_text_color(),
            )
        } else {
            (None, ui.visuals().weak_text_color())
        };
        if let Some(fill) = fill {
            painter.rect_filled(rect, CornerRadius::ZERO, fill);
        }
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            MDI_DOTS_HORIZONTAL,
            TextStyle::Body.resolve(ui.style()),
            color,
        );
        if response.has_focus() {
            focus_ring(ui, rect.shrink(2.0));
        }
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
/// returns its response: a dashed gray 500 outline around the words, with the square of the [`kind_hue`] of the group
/// `kind` before them on a card's add buttons.
///
/// The outline turns solid under the pointer and blue 500 while the menu it opens is `open`.
pub fn add_row_button(ui: &mut Ui, words: &str, name: &str, kind: Option<AddButton>, open: bool) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let color = ui.visuals().text_color();
    let galley = ui.painter().layout_no_wrap(words.to_owned(), font, color);
    let square = kind.map_or(0.0, |_| ADD_SQUARE + 5.0);
    let size = vec2(galley.size().x + square + 12.0, OVERLAY_ROW - 2.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if open {
            painter.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, mcs::BLUE_500),
                StrokeKind::Inside,
            );
        } else if response.hovered() {
            painter.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, mcs::GRAY_500),
                StrokeKind::Inside,
            );
        } else {
            painter.extend(dashed_rect(rect, Stroke::new(1.0, mcs::GRAY_500)));
        }
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

/// Adds the small button of a note, showing `words` and named `name`, and returns its response: a gray 700 outline
/// around the words at the note's size, gray 400 under the pointer.
pub fn note_button(ui: &mut Ui, words: &str, name: &str, size: f32) -> Response {
    let color = ui.visuals().text_color();
    let galley = ui
        .painter()
        .layout_no_wrap(words.to_owned(), FontId::proportional(size), color);
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(10.0, 4.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), name));
    if ui.is_rect_visible(rect) {
        let outline = if response.hovered() {
            mcs::GRAY_400
        } else {
            mcs::GRAY_700
        };
        let painter = ui.painter();
        painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, outline), StrokeKind::Inside);
        painter.galley(rect.min + vec2(5.0, 2.0), galley, color);
        if response.has_focus() {
            focus_ring(ui, rect);
        }
    }
    response
}

/// Adds one segment of a segmented toggle showing `words`, selected while `selected` holds, and
/// returns its response: weak text while off, lighter under the pointer, and the text colour over a 2 point blue 500
/// line while on.
///
/// The segment is a button on the keyboard path, which Space or Enter presses.
pub fn toggle_label(ui: &mut Ui, words: &str, selected: bool) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(words.to_owned(), font, Color32::PLACEHOLDER);
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(8.0, 6.0), Sense::click());
    response.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, selected, words));
    if ui.is_rect_visible(rect) {
        let color = if selected {
            ui.visuals().text_color()
        } else if response.hovered() {
            ui.visuals().widgets.hovered.text_color()
        } else {
            ui.visuals().weak_text_color()
        };
        let painter = ui.painter();
        painter.galley(rect.min + vec2(4.0, 2.0), galley, color);
        if selected {
            let line = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), rect.right_bottom());
            painter.rect_filled(line, CornerRadius::ZERO, mcs::BLUE_500);
        }
        if response.has_focus() {
            focus_ring(ui, rect);
        }
    }
    response
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

/// Kind of a notice, which picks the hue of its bar and its icon.
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

    /// Hue of the notice's bar and icon, at step 500.
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

/// A notice: a row with a 2 point bar and an icon in the hue of its kind before its text in the text colour, and any
/// small buttons after the text.
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
        let row = Frame::NONE
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
            });
        let rect = row.response.rect;
        let bar = Rect::from_min_size(rect.min, vec2(2.0, rect.height()));
        ui.painter().rect_filled(bar, CornerRadius::ZERO, self.kind.hue());
        row.inner
    }
}

// Dialogs

/// Margin in points between a dialog's border and what it holds.
const DIALOG_MARGIN: i8 = 12;

/// Returns the frame of a dialog: the theme's window fill inside the overlay's 1 point gray 600 border, with no shadow
/// and 12 points of margin.
pub fn dialog_frame(ctx: &egui::Context) -> Frame {
    Frame::popup(&ctx.global_style()).inner_margin(Margin::same(DIALOG_MARGIN))
}

/// Starts a dialog `width` points wide named `name` with the role of a dialog, and draws its title: `title` as a small
/// upper-case mono label with a gray 800 hairline under it across the dialog.
pub fn dialog_title(ui: &mut Ui, name: &str, title: &str, width: f32) {
    ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
        node.set_role(egui::accesskit::Role::Dialog);
        node.set_label(name);
    });
    ui.set_width(width);
    caption(ui, title);
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
/// The row takes the gray 800 fill under the pointer and the keyboard focus, and the focus outline inside its edge while
/// it holds the focus. A selected row takes the tinted fill, blue 900 with its name in blue 200 after a check, as the
/// current item of an overlay does.
pub fn choice_row(ui: &mut Ui, name: &str, description: Option<&str>, selected: bool) -> Response {
    let background = ui.painter().add(Shape::Noop);
    let weak = ui.visuals().weak_text_color();
    let name_color = if selected {
        mcs::BLUE_200
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
    if let Some(fill) = list_row_fill(selected, response.hovered() || response.has_focus()) {
        ui.painter()
            .set(background, Shape::rect_filled(row, CornerRadius::ZERO, fill));
    }
    paint_list_row_marks(ui.painter(), row, selected, response.has_focus(), name_rect.center().y);
    response
}

/// Returns the fill of a row of a list, an overlay's item or a dialog's choice: blue 900 while `chosen`, else gray 800
/// while `active`, under the pointer or the focus, and none otherwise.
fn list_row_fill(chosen: bool, active: bool) -> Option<Color32> {
    if chosen {
        Some(mcs::BLUE_900)
    } else if active {
        Some(mcs::GRAY_800)
    } else {
        None
    }
}

/// Paints the marks of `row`, a row of a list: the 1.5 point blue 500 focus stroke inside it while `focused`, and a
/// blue 200 check at its left, centred on `check_y`, while `chosen`.
fn paint_list_row_marks(painter: &Painter, row: Rect, chosen: bool, focused: bool, check_y: f32) {
    if focused {
        painter.rect_stroke(
            row,
            CornerRadius::ZERO,
            Stroke::new(FOCUS_WIDTH, mcs::BLUE_500),
            StrokeKind::Inside,
        );
    }
    if chosen {
        painter.text(
            pos2(row.left() + OVERLAY_PADDING, check_y),
            egui::Align2::LEFT_CENTER,
            MDI_CHECK,
            FontId::proportional(SMALL_LABEL),
            mcs::BLUE_200,
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

/// Colours of a chip: its fill, its outline, its name and its glyph or lead word.
struct ChipColors {
    /// Fill of the chip.
    fill: Color32,
    /// Outline of the chip, `None` for none.
    outline: Option<Color32>,
    /// Colour of the chip's name.
    text: Color32,
    /// Colour of the lead word and of the slider mark.
    lead: Color32,
}

/// Returns the colours of a chip of `kind`: red for a result that its line cannot use, blue for a result and for any
/// chip whose picker is `open`, with a blue 500 outline while open, and neutral otherwise.
fn chip_colors(kind: &ChipKind, red: bool, open: bool) -> ChipColors {
    let (fill, text, lead) = match (kind, red, open) {
        (_, true, _) => (mcs::RED_900, mcs::RED_200, mcs::RED_200),
        (ChipKind::Result(_), false, _) | (_, false, true) => (mcs::BLUE_900, mcs::BLUE_200, mcs::BLUE_200),
        _ => (mcs::GRAY_800, mcs::GRAY_100, mcs::GRAY_400),
    };
    ChipColors {
        fill,
        outline: open.then_some(mcs::BLUE_500),
        text,
        lead,
    }
}

/// Adds the chip of `chip`, a button named by its visible text, under the widget id `id` when one is given, so another
/// frame can open its picker or give it the focus, and returns its response.
///
/// A result chip is blue, or red and struck through when `red` holds, and every other chip neutral. A parameter chip
/// carries a slider mark, a state chip a swatch of the colour `swatch`, a property chip its lead word ("my"), "any
/// state" is in italics and a number in the mono face. Clicking a chip does nothing.
///
/// Under the pointer the chip takes a 1 point outline in its text colour, and while the picker of `id` is open the
/// blue tinted fill inside a blue 500 outline. The eye then finds what the picker edits.
pub fn chip_with_id(ui: &mut Ui, id: Option<Id>, chip: &SentenceChip, red: bool, swatch: Option<Color32>) -> Response {
    chip_within(ui, id, chip, red, swatch, f32::INFINITY)
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
    let colors = chip_colors(&chip.kind, red, open);
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
        response.on_hover_text(chip.text())
    } else {
        response
    };
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, chip.text()));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::ZERO, colors.fill);
        let outline = if response.hovered() && !open {
            Some(colors.text)
        } else {
            colors.outline
        };
        if let Some(outline) = outline {
            painter.rect_stroke(rect, CornerRadius::ZERO, Stroke::new(1.0, outline), StrokeKind::Inside);
        }
        let mut x = rect.left() + CHIP_PADDING;
        let center_y = rect.center().y;
        if let Some(color) = swatch {
            self::swatch(painter, Pos2::new(x + CHIP_SWATCH / 2.0, center_y), color);
            x += swatch_width;
        }
        for galley in [mark, lead].into_iter().flatten() {
            let size = galley.size();
            painter.galley(Pos2::new(x, center_y - size.y / 2.0), galley, colors.lead);
            x += size.x + CHIP_GAP;
        }
        let size = name.size();
        let name_rect = Rect::from_min_size(Pos2::new(x, center_y - size.y / 2.0), size);
        painter.galley(name_rect.min, name, colors.text);
        if red {
            painter.hline(name_rect.x_range(), center_y, Stroke::new(1.0, colors.text));
        }
        if response.has_focus() {
            focus_ring(ui, rect);
        }
    }
    response
}

/// Adds a dashed blank showing `word`, named `name`, and returns its response with the index of its shapes.
///
/// A blank with `painted` false is laid out and named but not painted, and returns the index of an empty shape that
/// the caller fills to paint it. The blank takes the widget id `id` when one is given. Under the pointer its outline
/// turns solid and its word gray 200, and while the picker of `id` is open it takes the blue tinted fill.
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
            painter.rect_filled(rect, CornerRadius::ZERO, mcs::BLUE_900);
            painter.add(blank_word(ui, rect, word, mcs::BLUE_200));
        } else if response.hovered() {
            painter.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, mcs::GRAY_500),
                StrokeKind::Inside,
            );
            painter.add(blank_word(ui, rect, word, mcs::GRAY_200));
        } else {
            painter.extend(blank_shapes(ui, rect, word, false));
        }
        if response.has_focus() {
            focus_ring(ui, rect);
        }
    }
    (None, response)
}

/// Returns the shapes of a rectangle outlined with dashes of `stroke` along the inside of `rect`.
pub fn dashed_rect(rect: Rect, stroke: Stroke) -> Vec<Shape> {
    let rect = rect.shrink(stroke.width / 2.0);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    Shape::dashed_line(&corners, stroke, 3.0, 2.0)
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

/// Returns the shapes of a dashed blank showing `word` in `rect`: gray 500 dashes around the word in gray 400, or
/// while `quiet` holds, as an optional blank is until its line is hovered, gray 700 dashes around the word in the weak
/// text colour.
pub fn blank_shapes(ui: &Ui, rect: Rect, word: &str, quiet: bool) -> Vec<Shape> {
    let (dashes, color) = if quiet {
        (mcs::GRAY_700, ui.visuals().weak_text_color())
    } else {
        (mcs::GRAY_500, mcs::GRAY_400)
    };
    let mut shapes = dashed_rect(rect, Stroke::new(1.0, dashes));
    shapes.push(blank_word(ui, rect, word, color));
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
/// which [`field_message`] returns. Typing alone changes nothing.
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
        commit,
    )
}

/// Adds a field of several lines named `name` that shows `current`, with the grey `hint` while it is empty, and commits
/// what is typed on losing focus.
///
/// Enter starts a new line. `commit` takes the typed text as [`commit_field`] does.
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

/// Adds the text field that `edit` makes over the text, named `name`, showing `current`, and commits what is typed on
/// losing focus through `commit`, as [`commit_field`] does.
fn commit_edit(
    ui: &mut Ui,
    id: Id,
    name: &str,
    current: &str,
    edit: impl for<'t> FnOnce(&'t mut String) -> TextEdit<'t>,
    commit: impl FnOnce(&str) -> Option<String>,
) -> Response {
    let draft: Option<FieldDraft> = ui.data(|data| data.get_temp(draft_id(id)));
    let mut text = draft
        .as_ref()
        .map_or_else(|| current.to_owned(), |draft| draft.text.clone());
    let response = ui.add(edit(&mut text).id(id));
    set_name(ui, response.id, name);
    let message = draft.and_then(|draft| draft.message);
    if response.lost_focus() {
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

/// Range a typed number is read in: a chance, a rate or any number. A picker's Number box and the New parameter dialog
/// accept numbers in it.
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
}

impl NumberUse {
    /// Returns the number typed as `text`, or the message under a text that does not read or lies outside the range.
    ///
    /// # Errors
    ///
    /// Returns the message for text that is not one number, a chance outside 0 to 1, or a rate below 0.
    pub fn read(self, text: &str) -> Result<SheetLiteral, &'static str> {
        let literal = read_typed_number(text).ok_or(TYPE_ONE_NUMBER)?;
        let number = literal.number().unwrap_or_default();
        match self {
            Self::Chance if !(0.0..=1.0).contains(&number) => Err(CHANCE_OUT_OF_RANGE),
            Self::Rate if number < 0.0 => Err(RATE_BELOW_ZERO),
            Self::Times if number < 0.0 || number.fract() != 0.0 => Err(TYPE_TIMES),
            _ => Ok(literal),
        }
    }

    /// Returns the hint over a Number box that reads in this range. It also names the box.
    pub fn box_hint(self) -> &'static str {
        match self {
            Self::Chance => CHANCE_BOX_HINT,
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
    let response = swatch_button(ui, color32(color), name, Popup::is_id_open(ui.ctx(), popup_id));
    let popup = Popup::from_toggle_button_response(&response)
        .id(popup_id)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .frame(Frame::NONE)
        .show(|ui| {
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
        })?;
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
/// The outline is gray 700, gray 100 under the pointer and blue 500 while the picker it opens is `open`. Keyboard focus
/// draws the focus outline.
fn swatch_button(ui: &mut Ui, color: Color32, name: &str, open: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(SWATCH_SIDE + 4.0, CHIP_HEIGHT), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let square = Rect::from_center_size(rect.center(), Vec2::splat(SWATCH_SIDE));
        let painter = ui.painter();
        painter.rect_filled(square, CornerRadius::ZERO, color);
        let outline = if open {
            mcs::BLUE_500
        } else if response.hovered() {
            mcs::GRAY_100
        } else {
            mcs::GRAY_700
        };
        painter.rect_stroke(
            square,
            CornerRadius::ZERO,
            Stroke::new(1.0, outline),
            egui::StrokeKind::Inside,
        );
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
    /// A chance or a rate starts from 0.1 in 0 to 1, any other number from 1 in 0 to 10, and a typed number from itself
    /// with a Max about ten times it.
    pub fn new(number_use: NumberUse, typed: Option<f64>, unit: Option<String>) -> Self {
        let (mut default, mut min, mut max) = match number_use {
            NumberUse::Chance | NumberUse::Rate => (0.1, 0.0, 1.0),
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
    /// default outside Min and Max, or a chance's range outside 0 to 1.
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
/// value below 1 or a chance.
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
    if size < 1.0 || number_use == NumberUse::Chance {
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
