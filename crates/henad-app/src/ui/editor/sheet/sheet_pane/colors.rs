//! The Colours section of the Sheet pane: the property that colours each kind of thing, and the shading of a Number
//! property.

use egui::{CornerRadius, Id, Rect, Sense, Ui, vec2};
use henad_graph::sheet::document::{
    CELLS_ID, ColorRule, ColorRuleKind, GROUND_ID, MadeOf, PropertyType, ShadeHue, SheetDocument, SheetProperty,
};
use henad_graph::sheet::edit::{SheetEdit, SheetItem};
use henad_graph::sheet::format_number;
use henad_graph::sheet::sentence::{
    BY_HEADING_NOTE, COLOR_AGENTS_BY, COLOR_BY_NOTHING, COLOR_GROUND_BY, state_swatch_name,
};
use henad_graph::sheet::sentence::{CHANGE, CHANGE_COLORS_HEADING};
use henad_graph::sheet::sentence::{
    COLOR_CELLS_BY, DARKEST_NAME, LIGHTEST_NAME, SHADE, SHADE_COLORS, SHADE_DARK_TO, SHADE_ENDS_EQUAL, SHADE_FROM,
    SHADE_LIGHT,
};

use super::layout::{Form, ITEM_GAP, text_width, wrapped_line};
use super::parameters::typed_number;
use super::{FIELD_WIDTH, GRADIENT_BANDS, GRADIENT_WIDTH, Pending, SWATCH_SIDE, measured_thing, select};
use super::{PaneState, edit_or_ask};
use crate::ui::editor::sheet::properties_of;
use crate::ui::editor::sheet::widgets::{
    OverlayItem, SWATCH_BUTTON_WIDTH, TypeSymbol, color_button, color_name, commit_number, field_message, message_ui,
    type_symbols_shown,
};

/// Returns the colour a number shaded in `hue` takes at `share` of the way from its darkest value to its lightest.
fn shade_color(hue: ShadeHue, share: f32) -> egui::Color32 {
    let [dark, light] = hue.ends();
    let channel = |index: usize| {
        let (low, high) = (f32::from(dark[index]), f32::from(light[index]));
        // The channels lie in 0 to 255, and so does every value between two of them.
        (low + (high - low) * share).round() as u8
    };
    egui::Color32::from_rgb(channel(0), channel(1), channel(2))
}

/// Returns the rule that shades the cells by `property`, keeping the hue and the range of `previous` when it shades
/// too, and blue from 0 to 1 otherwise.
pub(super) fn shaded_rule(property: &str, previous: Option<&ColorRule>) -> ColorRule {
    let mut rule = ColorRule::new(ColorRuleKind::Shaded);
    rule.property = Some(property.to_owned());
    let previous = previous.filter(|previous| previous.rule == ColorRuleKind::Shaded);
    rule.hue = Some(previous.and_then(|previous| previous.hue).unwrap_or(ShadeHue::Blue));
    rule.darkest = Some(previous.and_then(|previous| previous.darkest).unwrap_or(0.0));
    rule.lightest = Some(previous.and_then(|previous| previous.lightest).unwrap_or(1.0));
    rule
}

/// Returns the command that colours `thing` by `property`: by its states' colours for a States property, by heading
/// for a Heading, and shaded as [`shaded_rule`] says otherwise.
fn color_rule_edit(thing: &str, property: &SheetProperty, previous: Option<&ColorRule>) -> SheetEdit {
    let rule = match property.property_type {
        PropertyType::States => {
            let mut rule = ColorRule::new(ColorRuleKind::ByState);
            rule.property = Some(property.id.clone());
            rule
        }
        PropertyType::Heading => ColorRule::new(ColorRuleKind::ByHeading),
        _ => shaded_rule(&property.id, previous),
    };
    SheetEdit::SetColorRule {
        thing: thing.to_owned(),
        rule: Some(rule),
    }
}

/// Draws the Colours section: the property that colours the cells, or in a world of agents the property that colours
/// the agents and the one that colours the ground. Its widgets are keyed under the id `pane` holds, and a change of
/// colours that drops shades waits in `pane` for the dialog that asks first.
pub(super) fn colors(ui: &mut Ui, document: &SheetDocument, pane: &mut PaneState, pending: &mut Vec<Pending>) {
    let form = Form::new(ui, &[COLOR_CELLS_BY, COLOR_AGENTS_BY, COLOR_GROUND_BY, SHADE]);
    if document.world.made_of != MadeOf::Agents {
        colors_of(ui, document, CELLS_ID, COLOR_CELLS_BY, &form, pane, pending);
        return;
    }
    colors_of(
        ui,
        document,
        measured_thing(document),
        COLOR_AGENTS_BY,
        &form,
        pane,
        pending,
    );
    if !properties_of(document, GROUND_ID).is_empty() {
        ui.add_space(4.0);
        colors_of(ui, document, GROUND_ID, COLOR_GROUND_BY, &form, pane, pending);
    }
}

/// Returns the property that the world colours `thing` by, among `properties`, the properties of `thing` that can
/// colour, or `None` when no property colours `thing`.
///
/// That is the property of the colour rule of `thing`, or the Heading for a rule by heading. With no rule, the cells
/// and the agents take the colours of their first States property, and the ground is not drawn.
fn drawn_by<'a>(document: &SheetDocument, thing: &str, properties: &[&'a SheetProperty]) -> Option<&'a SheetProperty> {
    let first_of = |kind: PropertyType| {
        properties
            .iter()
            .find(|property| property.property_type == kind)
            .copied()
    };
    match document.colors.get(thing) {
        Some(rule) if rule.rule == ColorRuleKind::ByHeading => first_of(PropertyType::Heading),
        Some(rule) => {
            let id = rule.property.as_deref()?;
            properties.iter().find(|property| property.id == id).copied()
        }
        None if thing == GROUND_ID => None,
        None => first_of(PropertyType::States),
    }
}

/// Draws what colours `thing` as rows of `form`: `label` and a select of its properties, with the states' colours under
/// a States property, the note on a heading, and the hue and the range of the shading of a Number property.
///
/// The select shows the property the world colours `thing` by ([`drawn_by`]), or [`COLOR_BY_NOTHING`]. It offers
/// [`COLOR_BY_NOTHING`] first, which takes the rule out, wherever no rule leaves `thing` coloured by no property: on
/// the ground, and on cells or agents with no States property. Nothing shows while `thing` has no property that can
/// colour.
fn colors_of(
    ui: &mut Ui,
    document: &SheetDocument,
    thing: &str,
    label: &str,
    form: &Form,
    pane: &mut PaneState,
    pending: &mut Vec<Pending>,
) {
    let widgets = pane.widgets;
    let properties: Vec<&SheetProperty> = properties_of(document, thing)
        .iter()
        .filter(|property| {
            matches!(
                property.property_type,
                PropertyType::States | PropertyType::Number | PropertyType::WholeNumber | PropertyType::Heading
            )
        })
        .collect();
    if properties.is_empty() {
        return;
    }
    let rule = document.colors.get(thing);
    let chosen = drawn_by(document, thing, &properties);
    let nothing_offered = thing == GROUND_ID
        || !properties
            .iter()
            .any(|property| property.property_type == PropertyType::States);
    // The options in the order the list shows them, `None` for no property.
    let options: Vec<Option<&SheetProperty>> = nothing_offered
        .then_some(None)
        .into_iter()
        .chain(properties.iter().map(|property| Some(*property)))
        .collect();
    form.row(ui, label, |ui| {
        // Each property follows the symbol of its type while the symbols show, and None lines up with them.
        let symbols = type_symbols_shown(ui.ctx());
        let items: Vec<OverlayItem<'_>> = options
            .iter()
            .map(|option| OverlayItem {
                label: option.map_or(COLOR_BY_NOTHING, |property| property.name.as_str()),
                current: option.map(|property| &property.id) == chosen.map(|property| &property.id),
                symbol: option
                    .filter(|_| symbols)
                    .map(|property| TypeSymbol::of_property(property.property_type)),
                symbol_space: symbols,
                ..OverlayItem::default()
            })
            .collect();
        let shown = chosen.map_or(COLOR_BY_NOTHING, |property| property.name.as_str());
        let (_, picked) = select(ui, ("henad_sheet_color_by", thing), label, shown, &items);
        let Some(&picked) = picked.and_then(|index| options.get(index)) else {
            return;
        };
        if picked.map(|property| &property.id) == chosen.map(|property| &property.id) {
            return;
        }
        let edit = match picked {
            Some(property) => color_rule_edit(thing, property, rule),
            None => SheetEdit::SetColorRule {
                thing: thing.to_owned(),
                rule: None,
            },
        };
        // Shades of the rule replaced go with it, and no control shades by a second property again.
        let asked = (CHANGE_COLORS_HEADING, shown, CHANGE);
        edit_or_ask(document, edit, asked, None, &mut pane.delete_question, pending);
    });
    let Some(chosen) = chosen else {
        return;
    };
    match chosen.property_type {
        PropertyType::States => {
            form.under(ui, |ui| {
                wrapped_line(ui, |ui| {
                    // A state's swatch and name stay on one line. egui wraps no nested group, and the pair moves to
                    // the next line by hand.
                    let mut first = true;
                    for state in &chosen.states {
                        let width = SWATCH_BUTTON_WIDTH + ui.spacing().item_spacing.x + text_width(ui, &state.name);
                        if !first && width > ui.available_size_before_wrap().x {
                            ui.end_row();
                        }
                        first = false;
                        // The swatch opens "Choose a colour", as the state's swatch in the property form does.
                        let name = state_swatch_name(&chosen.name, &state.name, color_name(state.color));
                        if let Some(color) = color_button(ui, &name, state.color)
                            && color != state.color
                        {
                            pending.push(Pending::one(SheetEdit::Recolor {
                                item: SheetItem::State {
                                    property: chosen.id.clone(),
                                    state: state.id.clone(),
                                },
                                color,
                            }));
                        }
                        ui.weak(&state.name);
                        ui.add_space(ITEM_GAP);
                    }
                });
            });
        }
        PropertyType::Heading => {
            form.under(ui, |ui| ui.weak(BY_HEADING_NOTE));
        }
        _ => {
            let shading = shaded_rule(
                &chosen.id,
                rule.filter(|rule| rule.property.as_deref() == Some(&chosen.id)),
            );
            shading_ui(ui, thing, &shading, form, widgets, pending);
        }
    }
}

/// Draws the hue and the range of `shading`, the rule that shades `thing` by a Number property, as rows of `form`. A
/// new hue or end commits the rule with it. `widgets` is the id the pane's widgets are keyed under.
fn shading_ui(ui: &mut Ui, thing: &str, shading: &ColorRule, form: &Form, widgets: Id, pending: &mut Vec<Pending>) {
    let hue = shading.hue.unwrap_or(ShadeHue::Blue);
    let hue_words = |hue: ShadeHue| {
        let index = ShadeHue::ALL.iter().position(|known| *known == hue).unwrap_or_default();
        SHADE_COLORS.get(index).copied().unwrap_or_default()
    };
    form.row(ui, SHADE, |ui| {
        let items: Vec<OverlayItem<'_>> = ShadeHue::ALL
            .iter()
            .map(|known| OverlayItem {
                label: hue_words(*known),
                current: *known == hue,
                ..OverlayItem::default()
            })
            .collect();
        let (_, chosen) = select(ui, ("henad_sheet_shade", thing), SHADE, hue_words(hue), &items);
        if let Some(&picked) = chosen.and_then(|index| ShadeHue::ALL.get(index))
            && picked != hue
        {
            let mut rule = shading.clone();
            rule.hue = Some(picked);
            pending.push(Pending::one(SheetEdit::SetColorRule {
                thing: thing.to_owned(),
                rule: Some(rule),
            }));
        }
        let (rect, _) = ui.allocate_exact_size(vec2(GRADIENT_WIDTH, SWATCH_SIDE), Sense::hover());
        let band = rect.width() / GRADIENT_BANDS as f32;
        for index in 0..GRADIENT_BANDS {
            let left = rect.left() + band * index as f32;
            let share = index as f32 / (GRADIENT_BANDS - 1) as f32;
            ui.painter().rect_filled(
                Rect::from_min_size(egui::pos2(left, rect.top()), vec2(band.ceil(), rect.height())),
                CornerRadius::ZERO,
                shade_color(hue, share),
            );
        }
    });
    let darkest_id = widgets.with(("henad_sheet_shade_darkest", thing));
    let lightest_id = widgets.with(("henad_sheet_shade_lightest", thing));
    // The ends left out read as 0 and 1, as the lowering reads them.
    let (darkest_at, lightest_at) = (shading.darkest.unwrap_or(0.0), shading.lightest.unwrap_or(1.0));
    form.under(ui, |ui| {
        wrapped_line(ui, |ui| {
            ui.label(SHADE_FROM);
            for (id, name, current, other, darkest) in [
                (darkest_id, DARKEST_NAME, darkest_at, lightest_at, true),
                (lightest_id, LIGHTEST_NAME, lightest_at, darkest_at, false),
            ] {
                commit_number(
                    ui,
                    id,
                    name,
                    &format_number(current).unwrap_or_default(),
                    FIELD_WIDTH,
                    |text| match typed_number(text) {
                        // The lowering refuses two equal ends, so the field refuses them first.
                        Ok(value) if value == other => Some(SHADE_ENDS_EQUAL.to_owned()),
                        Ok(value) => {
                            let mut rule = shading.clone();
                            if darkest {
                                rule.darkest = Some(value);
                            } else {
                                rule.lightest = Some(value);
                            }
                            pending.push(Pending::one(SheetEdit::SetColorRule {
                                thing: thing.to_owned(),
                                rule: Some(rule),
                            }));
                            None
                        }
                        Err(message) => Some(message),
                    },
                );
                ui.label(if darkest { SHADE_DARK_TO } else { SHADE_LIGHT });
            }
        });
        if let Some(message) = field_message(ui, darkest_id).or_else(|| field_message(ui, lightest_id)) {
            message_ui(ui, &message);
        }
    });
}
