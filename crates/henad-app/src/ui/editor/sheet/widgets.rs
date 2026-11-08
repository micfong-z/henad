//! Controls the Sheet pane and the Card pane share: the chip, the dashed blank, a number field that commits on Enter
//! or on losing focus, the picker, the colour palette and the New parameter dialog.
//!
//! A *picker* is a popover under its anchor that lists what a blank or a field can take, in groups under headings, with
//! "New parameter..." and a box for a typed number. A group with no item is left out, and a picker with nothing to
//! offer says what is missing. A typed number takes a trailing "%" as a percent ("30%" is 0.3).

use egui::{
    Button, Color32, CornerRadius, Id, Key, Modal, Popup, PopupCloseBehavior, Pos2, Rect, Response, RichText, Sense,
    Shape, Stroke, TextEdit, TextStyle, Ui, Vec2, WidgetInfo, WidgetType, vec2,
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
use crate::icons::material_design_icons::MDI_TUNE_VERTICAL;
use crate::ui::mcs;
use crate::ui::sweep::layout::filled_button;

/// Padding in points inside a chip, a blank or a select.
pub const CHIP_PADDING: Vec2 = vec2(4.0, 1.0);

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

/// Colours of a chip: its fill, its text and its grey lead word.
struct ChipColors {
    /// Fill of the chip.
    fill: Color32,
    /// Colour of the chip's name.
    text: Color32,
    /// Colour of the lead word and of the slider mark.
    lead: Color32,
}

/// Returns the colours of a chip of `kind`, red for a result that its line cannot use.
fn chip_colors(kind: &ChipKind, red: bool) -> ChipColors {
    match (kind, red) {
        (_, true) => ChipColors {
            fill: mcs::RED_900,
            text: mcs::RED_200,
            lead: mcs::RED_300,
        },
        (ChipKind::Result(_), false) => ChipColors {
            fill: mcs::BLUE_900,
            text: mcs::BLUE_200,
            lead: mcs::BLUE_300,
        },
        _ => ChipColors {
            fill: mcs::GRAY_800,
            text: mcs::GRAY_100,
            lead: mcs::GRAY_400,
        },
    }
}

/// Adds the chip of `chip`, a button named by its visible text, and returns its response.
///
/// A result chip fills blue, or red when `red` holds. A parameter chip carries a slider mark, a state chip a swatch of
/// the colour `swatch`, and a property chip its lead word ("my") in grey. Clicking a chip does nothing.
pub fn chip_ui(ui: &mut Ui, chip: &SentenceChip, red: bool, swatch: Option<Color32>) -> Response {
    chip_with_id(ui, None, chip, red, swatch)
}

/// Adds the chip of `chip` as [`chip_ui`] does, under the widget id `id` when one is given, so another frame can open
/// its picker or give it the focus.
pub fn chip_with_id(ui: &mut Ui, id: Option<Id>, chip: &SentenceChip, red: bool, swatch: Option<Color32>) -> Response {
    let colors = chip_colors(&chip.kind, red);
    let font = TextStyle::Body.resolve(ui.style());
    let painter = ui.painter();
    let mark = matches!(chip.kind, ChipKind::Parameter(_))
        .then(|| painter.layout_no_wrap(format!("{MDI_TUNE_VERTICAL} "), font.clone(), colors.lead));
    let lead =
        (!chip.lead.is_empty()).then(|| painter.layout_no_wrap(format!("{} ", chip.lead), font.clone(), colors.lead));
    let name = painter.layout_no_wrap(chip.name.clone(), font.clone(), colors.text);
    let swatch_side = font.size * 0.75;
    let swatch_width = if swatch.is_some() { swatch_side + 4.0 } else { 0.0 };
    let width = CHIP_PADDING.x * 2.0
        + swatch_width
        + mark.as_ref().map_or(0.0, |galley| galley.size().x)
        + lead.as_ref().map_or(0.0, |galley| galley.size().x)
        + name.size().x;
    let height = (name.size().y + CHIP_PADDING.y * 2.0).max(ui.spacing().interact_size.y - 2.0);
    let (rect, response) = allocate_control(ui, id, vec2(width, height));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, chip.text()));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::ZERO, colors.fill);
        if response.has_focus() || response.hovered() {
            painter.rect_stroke(
                rect,
                CornerRadius::ZERO,
                Stroke::new(1.0, colors.text),
                egui::StrokeKind::Inside,
            );
        }
        let mut x = rect.left() + CHIP_PADDING.x;
        let center_y = rect.center().y;
        if let Some(color) = swatch {
            let square = Rect::from_center_size(Pos2::new(x + swatch_side / 2.0, center_y), Vec2::splat(swatch_side));
            painter.rect_filled(square, CornerRadius::ZERO, color);
            painter.rect_stroke(
                square,
                CornerRadius::ZERO,
                Stroke::new(1.0, mcs::GRAY_600),
                egui::StrokeKind::Inside,
            );
            x += swatch_width;
        }
        for galley in [mark, lead, Some(name)].into_iter().flatten() {
            let size = galley.size();
            painter.galley(Pos2::new(x, center_y - size.y / 2.0), galley, colors.text);
            x += size.x;
        }
    }
    response
}

/// Adds a dashed blank showing `word`, named `name`, and returns its response with the index of its shapes.
///
/// A blank with `painted` false is laid out and named but not painted, and returns the index of an empty shape that
/// the caller fills to paint it. The blank takes the widget id `id` when one is given.
pub fn blank_ui(
    ui: &mut Ui,
    id: Option<Id>,
    word: &str,
    name: &str,
    painted: bool,
) -> (Option<egui::layers::ShapeIdx>, Response) {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(word.to_owned(), font, mcs::GRAY_400);
    let size = galley.size() + CHIP_PADDING * 2.0;
    let height = size.y.max(ui.spacing().interact_size.y - 2.0);
    let (rect, response) = allocate_control(ui, id, vec2(size.x, height));
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    if painted {
        if ui.is_rect_visible(rect) {
            let shapes = blank_shapes(ui, rect, word);
            ui.painter().extend(shapes);
            if response.has_focus() {
                ui.painter().rect_stroke(
                    rect,
                    CornerRadius::ZERO,
                    Stroke::new(1.0, mcs::GRAY_200),
                    egui::StrokeKind::Inside,
                );
            }
        }
        (None, response)
    } else {
        (Some(ui.painter().add(Shape::Noop)), response)
    }
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

/// Returns the shapes of a dashed blank showing `word` in `rect`.
pub fn blank_shapes(ui: &Ui, rect: Rect, word: &str) -> Vec<Shape> {
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(word.to_owned(), font, mcs::GRAY_400);
    let stroke = Stroke::new(1.0, mcs::GRAY_500);
    let corners = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom(),
        rect.left_bottom(),
        rect.left_top(),
    ];
    let mut shapes = Shape::dashed_line(&corners, stroke, 3.0, 2.0);
    let size = galley.size();
    shapes.push(Shape::galley(
        Pos2::new(rect.left() + CHIP_PADDING.x, rect.center().y - size.y / 2.0),
        galley,
        mcs::GRAY_400,
    ));
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
