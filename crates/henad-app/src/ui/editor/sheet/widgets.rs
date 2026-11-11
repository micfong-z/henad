//! Controls the Sheet pane and the Card pane share: the chip, the dashed blank, a number field that commits on Enter
//! or on losing focus, the picker, the colour palette and the New parameter dialog.
//!
//! A *picker* is a popover under its anchor that lists what a blank or a field can take, in groups under headings, with
//! "New parameter..." and a box for a typed number. A group with no item is left out, and a picker with nothing to
//! offer says what is missing. A typed number takes a trailing "%" as a percent ("30%" is 0.3).

use egui::{
    Button, Color32, CornerRadius, FontId, Frame, Id, Key, Margin, Modal, Popup, PopupCloseBehavior, Pos2, Rect,
    Response, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, TextStyle, Ui, Vec2, WidgetInfo, WidgetType, pos2,
    vec2,
};
use henad_graph::sheet::document::{ParameterType, SheetLiteral};
use henad_graph::sheet::edit::{IdSpace, PALETTE_COLORS, SheetEditError, is_reserved_name};
use henad_graph::sheet::format_number;
use henad_graph::sheet::sentence::{
    CANCEL, CHANCE_BOX_HINT, CHANCE_LIMITS_OUTSIDE, CHANCE_OUT_OF_RANGE, CHANCE_PARAMETER_PLACEHOLDER, CHOOSE_A_COLOR,
    COLOR_NAMES, CREATE_PARAMETER, ChipKind, DEFAULT_LABEL, DEFAULT_NAME, DEFAULT_OUTSIDE_LIMITS, DIALOG_NAME_MISSING,
    DIALOG_UNIT_PLACEHOLDER, LIMITS_NOT_NUMBERS, MAX_LABEL, MIN_LABEL, MIN_NOT_BELOW_MAX, NAME_LABEL, NEW_PARAMETER,
    NEW_PARAMETER_HEADING, NEW_PARAMETER_LEAD, NUMBER_BOX_HINT, NUMBER_PARAMETER_PLACEHOLDER, PARAMETER_NAME_FIELD,
    RATE_BELOW_ZERO, RATE_PARAMETER_PLACEHOLDER, SentenceChip, TIMES_LIMITS_NOT_INTEGERS, TIMES_PARAMETER_PLACEHOLDER,
    TYPE_ONE_NUMBER, TYPE_TIMES, UNIT_LABEL, USE_BUTTON, parameter_name_taken, read_typed_number,
};
use henad_graph::value::Color;

use super::accessible_group;
use crate::icons::material_design_icons::{
    MDI_CHECK, MDI_CHEVRON_DOWN, MDI_DOTS_HORIZONTAL, MDI_INFORMATION, MDI_TUNE_VERTICAL,
};
use crate::ui::mcs;
use crate::ui::sweep::layout::filled_button;

/// Padding in points at each side of the words of a chip, a blank or a select.
pub const CHIP_PADDING: f32 = 5.0;

/// Height in points of a chip, a blank or a select.
pub const CHIP_HEIGHT: f32 = 20.0;

/// Gap in points between a chip's glyph, swatch or lead word and its name.
const CHIP_GAP: f32 = 4.0;

/// Side in points of the swatch of a state chip.
const CHIP_SWATCH: f32 = 9.0;

/// Width in points of the keyboard focus outline.
const FOCUS_WIDTH: f32 = 1.5;

/// Padding in points inside an overlay, and at the sides of its items.
const OVERLAY_PADDING: f32 = 6.0;

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

// Custom components
//
// Each control the sheet editor paints itself is drawn by one function here, and no call site passes a colour. The
// colours follow the palette in `agents/ui.md`: a tinted fill is a hue's 900 with its 200 for text, a line or an
// outline in a hue is its 500, a neutral chip is gray 800 with gray 100 text, a neutral outline gray 700, an empty slot
// gray 500 dashes around a gray 400 word, a hairline gray 800, and an overlay the window fill inside gray 600.

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
        let (fill, text_color) = if !enabled {
            (None, visuals.weak_text_color())
        } else if item.current {
            (Some(mcs::BLUE_900), mcs::BLUE_200)
        } else if active {
            (Some(mcs::GRAY_800), visuals.widgets.hovered.text_color())
        } else {
            (None, visuals.widgets.inactive.text_color())
        };
        let painter = ui.painter();
        if let Some(fill) = fill {
            painter.rect_filled(row, CornerRadius::ZERO, fill);
        }
        if response.has_focus() {
            painter.rect_stroke(
                row,
                CornerRadius::ZERO,
                Stroke::new(FOCUS_WIDTH, mcs::BLUE_500),
                StrokeKind::Inside,
            );
        }
        let top = row.top() + (height - content) / 2.0;
        let line_center = top + label_size.y / 2.0;
        if item.current {
            painter.text(
                pos2(row.left() + OVERLAY_PADDING, line_center),
                egui::Align2::LEFT_CENTER,
                MDI_CHECK,
                FontId::proportional(SMALL_LABEL),
                mcs::BLUE_200,
            );
        }
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
    let font = TextStyle::Body.resolve(ui.style());
    let color = if empty { mcs::GRAY_400 } else { mcs::GRAY_100 };
    let weak = ui.visuals().weak_text_color();
    let painter = ui.painter();
    let text = painter.layout_no_wrap(words.to_owned(), font, color);
    let chevron = painter.layout_no_wrap(MDI_CHEVRON_DOWN.to_owned(), FontId::proportional(SMALL_LABEL), weak);
    let width = CHIP_PADDING + text.size().x + CHIP_GAP + chevron.size().x + 3.0;
    let (rect, response) = allocate_control(ui, id, vec2(width, CHIP_HEIGHT));
    response.widget_info(|| WidgetInfo {
        current_text_value: (!words.is_empty()).then(|| words.to_owned()),
        ..WidgetInfo::labeled(
            if words.is_empty() {
                WidgetType::Button
            } else {
                WidgetType::ComboBox
            },
            true,
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

/// Adds an add button showing `words`, named `name`, and returns its response: a dashed gray 500 outline around the
/// words, with the square of the kind's colour `kind` before them on a card's add buttons.
///
/// The outline turns solid under the pointer and blue 500 while the menu it opens is `open`.
pub fn add_button(ui: &mut Ui, words: &str, name: &str, kind: Option<Color32>, open: bool) -> Response {
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
            painter.rect_filled(square, CornerRadius::ZERO, kind);
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
/// A `focusable` segment is a button on the keyboard path. Any other segment is a label that a click still presses.
pub fn toggle_label(ui: &mut Ui, words: &str, selected: bool, focusable: bool) -> Response {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(words.to_owned(), font, Color32::PLACEHOLDER);
    let sense = if focusable { Sense::click() } else { Sense::CLICK };
    let (rect, response) = ui.allocate_exact_size(galley.size() + vec2(8.0, 6.0), sense);
    response.widget_info(|| {
        if focusable {
            WidgetInfo::selected(WidgetType::Button, true, selected, words)
        } else {
            WidgetInfo::labeled(WidgetType::Label, true, words)
        }
    });
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
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, height), Sense::hover());
    ui.painter()
        .vline(rect.center().x, rect.y_range(), Stroke::new(1.0, mcs::GRAY_800));
}

/// Adds an information notice: a row with a 2 point blue 500 bar and an information icon in blue 500 before `text` in
/// the text colour. The row reads as one label, the icon, a space and the text.
pub fn notice_ui(ui: &mut Ui, text: &str) {
    let font = TextStyle::Body.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();
    job.append(
        MDI_INFORMATION,
        0.0,
        egui::TextFormat {
            font_id: font.clone(),
            color: mcs::BLUE_500,
            ..Default::default()
        },
    );
    job.append(
        &format!(" {text}"),
        0.0,
        egui::TextFormat {
            font_id: font,
            color: ui.visuals().text_color(),
            ..Default::default()
        },
    );
    let row = Frame::NONE
        .inner_margin(Margin {
            left: 8,
            right: 4,
            top: 3,
            bottom: 3,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(job).wrap());
        })
        .response
        .rect;
    let bar = Rect::from_min_size(row.min, vec2(2.0, row.height()));
    ui.painter().rect_filled(bar, CornerRadius::ZERO, mcs::BLUE_500);
}

/// Width in points of a picker.
const PICKER_WIDTH: f32 = 280.0;

/// Side in points of a colour swatch button.
const SWATCH_SIDE: f32 = 16.0;

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
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), characters.as_str()),
        None => text,
    }
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

/// Adds the chip of `chip`, a button named by its visible text, and returns its response.
///
/// A result chip is blue, or red and struck through when `red` holds, and every other chip neutral. A parameter chip
/// carries a slider mark, a state chip a swatch of the colour `swatch`, a property chip its lead word ("my"), "any
/// state" is in italics and a number in the mono face. Clicking a chip does nothing.
pub fn chip_ui(ui: &mut Ui, chip: &SentenceChip, red: bool, swatch: Option<Color32>) -> Response {
    chip_with_id(ui, None, chip, red, swatch)
}

/// Adds the chip of `chip` as [`chip_ui`] does, under the widget id `id` when one is given, so another frame can open
/// its picker or give it the focus.
///
/// Under the pointer the chip takes a 1 point outline in its text colour, and while the picker of `id` is open the
/// blue tinted fill inside a blue 500 outline. The eye then finds what the picker edits.
pub fn chip_with_id(ui: &mut Ui, id: Option<Id>, chip: &SentenceChip, red: bool, swatch: Option<Color32>) -> Response {
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
    let name = painter.layout_job(job);
    let swatch_width = if swatch.is_some() { CHIP_SWATCH + CHIP_GAP } else { 0.0 };
    let width = CHIP_PADDING * 2.0
        + swatch_width
        + mark.as_ref().map_or(0.0, |galley| galley.size().x + CHIP_GAP)
        + lead.as_ref().map_or(0.0, |galley| galley.size().x + CHIP_GAP)
        + name.size().x;
    let (rect, response) = allocate_control(ui, id, vec2(width, CHIP_HEIGHT));
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

/// Draws `message` in the theme's error colour under a row of fields.
pub fn message_ui(ui: &mut Ui, message: &str) {
    ui.colored_label(ui.visuals().error_fg_color, message);
}

// The picker

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
}

/// One group of a picker: a heading and its items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerGroup {
    /// Heading of the group.
    pub heading: &'static str,
    /// Items of the group, in order.
    pub items: Vec<PickerItem>,
}

/// One item of a picker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerItem {
    /// Words of the item. They also name it.
    pub label: String,
    /// Grey words beside the item, such as a parameter's value and unit.
    pub detail: Option<String>,
}

/// Choice made in a picker.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PickerChoice {
    /// The item at `item` of the group at `group`, counted among every group passed to the picker.
    Item {
        /// Index of the group.
        group: usize,
        /// Index of the item in its group.
        item: usize,
    },
    /// "New parameter...".
    NewParameter,
    /// A number typed in the Number box.
    Number(SheetLiteral),
}

/// A picker: its title, its groups, "New parameter..." and its Number box.
#[derive(Debug, Clone)]
pub struct Picker<'a> {
    /// Title of the picker, such as "Choose a parameter".
    pub title: &'a str,
    /// Groups of items. A group with no item is left out.
    pub groups: Vec<PickerGroup>,
    /// Whether the picker offers "New parameter...".
    pub new_parameter: bool,
    /// Range the Number box reads, `None` for a picker with no Number box.
    pub number_box: Option<NumberUse>,
    /// Text the picker shows when it has nothing to offer.
    pub empty: &'a str,
}

impl Picker<'_> {
    /// Draws the picker under `anchor` while it is open, and returns what the author chose. A click on `anchor` opens
    /// and closes it, and a choice, a click outside or Escape closes it.
    pub fn show(self, ui: &Ui, anchor: &Response) -> Option<PickerChoice> {
        let popup = Popup::from_toggle_button_response(anchor)
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .width(PICKER_WIDTH)
            .show(|ui| {
                let choice = self.contents(ui);
                if choice.is_some() {
                    ui.close();
                }
                (choice, ui.unique_id())
            })?;
        let (choice, id) = popup.inner;
        accessible_group(
            ui.ctx(),
            id,
            egui::accesskit::Role::Dialog,
            self.title,
            popup.response.rect,
        );
        choice
    }

    /// Draws what the picker offers, and returns what the author chose.
    fn contents(&self, ui: &mut Ui) -> Option<PickerChoice> {
        let mut choice = None;
        ui.strong(self.title);
        let mut offered = false;
        for (group_index, group) in self.groups.iter().enumerate() {
            if group.items.is_empty() {
                continue;
            }
            offered = true;
            ui.add_space(4.0);
            ui.label(RichText::new(group.heading).weak().small());
            for (item_index, item) in group.items.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.add(Button::new(&item.label).frame(false)).clicked() {
                        choice = Some(PickerChoice::Item {
                            group: group_index,
                            item: item_index,
                        });
                    }
                    if let Some(detail) = &item.detail {
                        ui.weak(detail);
                    }
                });
            }
        }
        if self.new_parameter {
            offered = true;
            if ui.add(Button::new(NEW_PARAMETER).frame(false)).clicked() {
                choice = Some(PickerChoice::NewParameter);
            }
        }
        if let Some(number_use) = self.number_box {
            offered = true;
            ui.add_space(4.0);
            if let Some(literal) = number_box(ui, number_use) {
                choice = Some(PickerChoice::Number(literal));
            }
        }
        if !offered {
            ui.weak(self.empty);
        }
        choice
    }
}

/// Draws the Number box of a picker, and returns the number typed once Enter or Use is pressed on text that reads.
fn number_box(ui: &mut Ui, number_use: NumberUse) -> Option<SheetLiteral> {
    let hint = match number_use {
        NumberUse::Chance => CHANCE_BOX_HINT,
        NumberUse::Rate | NumberUse::Number | NumberUse::Times => NUMBER_BOX_HINT,
    };
    ui.weak(hint);
    let id = ui.id().with("henad_number_box");
    let mut draft: FieldDraft = ui.data(|data| data.get_temp(id)).unwrap_or_default();
    let mut typed = None;
    ui.horizontal(|ui| {
        let field = ui.add(TextEdit::singleline(&mut draft.text).desired_width(120.0));
        set_name(ui, field.id, hint);
        let entered = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        if ui.button(USE_BUTTON).clicked() || entered {
            match number_use.read(&draft.text) {
                Ok(literal) => {
                    typed = Some(literal);
                    draft = FieldDraft::default();
                }
                Err(message) => draft.message = Some(message.to_owned()),
            }
        }
    });
    if let Some(message) = &draft.message {
        message_ui(ui, message);
    }
    ui.data_mut(|data| data.insert_temp(id, draft));
    typed
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
pub fn color_button(ui: &mut Ui, name: &str, color: Color) -> Option<Color> {
    let response = swatch_button(ui, color32(color), name);
    let popup = Popup::from_toggle_button_response(&response)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.strong(CHOOSE_A_COLOR);
            let mut chosen = None;
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(6.0 * (SWATCH_SIDE + 8.0));
                for (palette, name) in PALETTE_COLORS.iter().zip(COLOR_NAMES) {
                    if swatch_button(ui, color32(*palette), name).on_hover_text(name).clicked() {
                        chosen = Some(*palette);
                    }
                }
            });
            if chosen.is_some() {
                ui.close();
            }
            (chosen, ui.unique_id())
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

/// Adds a square swatch of `color`, a button named `name`.
fn swatch_button(ui: &mut Ui, color: Color32, name: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(SWATCH_SIDE + 4.0), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if ui.is_rect_visible(rect) {
        let square = Rect::from_center_size(rect.center(), Vec2::splat(SWATCH_SIDE));
        let painter = ui.painter();
        painter.rect_filled(square, CornerRadius::ZERO, color);
        let outline = if response.hovered() || response.has_focus() {
            mcs::GRAY_100
        } else {
            mcs::GRAY_600
        };
        painter.rect_stroke(
            square,
            CornerRadius::ZERO,
            Stroke::new(1.0, outline),
            egui::StrokeKind::Inside,
        );
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
        if taken.iter().any(|known| known.to_lowercase() == name.to_lowercase()) {
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
    let response = Modal::new(Id::new("henad_new_parameter_modal")).show(ctx, |ui| {
        ui.ctx().accesskit_node_builder(ui.unique_id(), |node| {
            node.set_role(egui::accesskit::Role::Dialog);
            node.set_label(NEW_PARAMETER_HEADING);
        });
        ui.set_max_width(DIALOG_WIDTH);
        ui.heading(NEW_PARAMETER_HEADING);
        ui.label(NEW_PARAMETER_LEAD);
        ui.add_space(6.0);
        let placeholder = match dialog.number_use {
            NumberUse::Chance => CHANCE_PARAMETER_PLACEHOLDER,
            NumberUse::Rate => RATE_PARAMETER_PLACEHOLDER,
            NumberUse::Number => NUMBER_PARAMETER_PLACEHOLDER,
            NumberUse::Times => TIMES_PARAMETER_PLACEHOLDER,
        };
        egui::Grid::new("henad_new_parameter_fields")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(NAME_LABEL);
                let name = ui.add(TextEdit::singleline(&mut dialog.name).hint_text(placeholder));
                set_name(ui, name.id, PARAMETER_NAME_FIELD);
                if std::mem::take(&mut dialog.focus_name) {
                    name.request_focus();
                }
                if name.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                    create = true;
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
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.button(CANCEL).clicked() {
                canceled = true;
            }
            if ui.add(filled_button(CREATE_PARAMETER.to_owned())).clicked() {
                create = true;
            }
        });
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
