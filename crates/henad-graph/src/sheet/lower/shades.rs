//! Colours of the ground: the view pass `colours_ground`.
//!
//! A States property of the ground takes the colours of its states, with its shade state shaded by the largest of its
//! shades in [`SHADE_BANDS`] bands each, and a Number property is shaded as [`super::colors::shade`] does.

use super::colors;
use super::paint::{expression, expression_number};
use super::value::{number, output};
use super::{Builder, Site, text};
use crate::document::Palette;
use crate::sheet::check::Place;
use crate::sheet::document::{ColorRuleKind, PropertyType, ShadeScale, SheetProperty};
use crate::types::ValueType;
use crate::value::{InputValue, Literal};

/// Number of bands each shade of the ground takes in its palette.
const SHADE_BANDS: u32 = 6;

/// Smallest value a shade on a log scale takes the logarithm of.
const SMALLEST_SHADE: f32 = 1e-30;

/// Writes the view pass of the ground's colours: the colours of a States property of the ground, the cells in its
/// shade state shaded by the largest of its shades, or a Number property shaded as [`colors::shade`] does.
pub(super) fn colors(builder: &mut Builder<'_>) {
    let document = builder.document;
    let Some(rule) = document.colors.get("ground") else {
        return;
    };
    let place = Place::Color("ground".to_owned());
    let outcome = match rule.rule {
        ColorRuleKind::ByState => by_state(builder, rule),
        ColorRuleKind::Shaded if rule.scale != Some(ShadeScale::Log) && rule.decades.is_none() => {
            match rule.property.as_deref().and_then(|id| builder.property(id)) {
                Some((_, property)) if property.property_type == PropertyType::Number => {
                    let site = colors::color_site(builder);
                    builder
                        .read(site.pass, &property.id)
                        .and_then(|read| colors::shade(builder, &site, &read, &property.id, rule))
                }
                _ => Err("the colour rule must shade a Number property of the ground".to_owned()),
            }
        }
        ColorRuleKind::Shaded => {
            Err("the colour rule of the ground cannot shade on a log scale: only shades of a state can".to_owned())
        }
        ColorRuleKind::ByHeading => Err("the ground has no heading to colour it by".to_owned()),
    };
    if let Err(message) = outcome {
        builder.problem(message, place);
    }
}

/// Writes the colours of the ground by the states of a property, its shade state shaded by its shades.
///
/// # Errors
///
/// Returns the message for a rule that names no States property of the ground, a shade of no Number property, or a
/// palette id that a category takes.
fn by_state(builder: &mut Builder<'_>, rule: &crate::sheet::document::ColorRule) -> Result<(), String> {
    let (_, property) = rule
        .property
        .as_deref()
        .and_then(|id| builder.property(id))
        .filter(|(_, property)| property.property_type == PropertyType::States)
        .ok_or("the colour rule must name a States property of the ground")?;
    let site = colors::color_site(builder);
    let read = builder.read(site.pass, &property.id)?;
    let category = ValueType::Category(property.id.clone());
    let read = builder.input_as(&site, &read, &category)?;
    let shade_state = match (&rule.shade_state, rule.shades.is_empty()) {
        (None, true) => {
            builder.emit(
                &site,
                "view.color",
                vec![("palette", text(&property.id))],
                vec![("index".to_owned(), read)],
            );
            return Ok(());
        }
        (Some(state), false) => state.as_str(),
        _ => return Err("the colour rule needs both a shaded state and its shades, or neither".to_owned()),
    };
    let shaded_index = property
        .states
        .iter()
        .position(|state| state.id == shade_state)
        .ok_or_else(|| format!("'{shade_state}' is not a state of '{}'", property.id))?;
    let palette_id = format!("shades_{}", property.id);
    if builder
        .graph
        .categories
        .iter()
        .any(|category| category.id == palette_id)
    {
        return Err(format!(
            "the shades palette id '{palette_id}' is already used by a property"
        ));
    }
    let colors = shade_palette(property, rule, shaded_index);
    builder.graph.palettes.push(Palette::new(&palette_id, colors));
    let (best, offset) = largest_shade(builder, &site, rule, property.states.len())?;
    let band = shade_band(builder, &site, best, rule.scale == Some(ShadeScale::Log), rule.decades);
    let shaded = shade_entry(builder, &site, band, offset, shaded_index as u32);
    let mut cases = Vec::with_capacity(property.states.len());
    for (index, state) in property.states.iter().enumerate() {
        let entry = if index == shaded_index {
            shaded.clone()
        } else {
            InputValue::Literal(Literal::U32(index as u32))
        };
        cases.push((state.id.as_str(), entry));
    }
    let index = builder.switch(&site, &property.id, read, cases);
    builder.emit(
        &site,
        "view.color",
        vec![("palette", text(&palette_id))],
        vec![("index".to_owned(), output(index))],
    );
    Ok(())
}

/// Returns the palette of a States property `property` shaded by `rule`: each state's colour, then for each shade
/// [`SHADE_BANDS`] colours that blend from the shade state's colour, the state at `shaded_index`, to the shade's.
fn shade_palette(
    property: &SheetProperty,
    rule: &crate::sheet::document::ColorRule,
    shaded_index: usize,
) -> Vec<[u8; 4]> {
    let base = property.states[shaded_index].color;
    let mut colors: Vec<[u8; 4]> = property.states.iter().map(|state| state.color).collect();
    for shade in &rule.shades {
        for band in 1..=SHADE_BANDS {
            let share = band as f32 / SHADE_BANDS as f32;
            colors.push(std::array::from_fn(|channel| {
                let (low, high) = (f32::from(base[channel]), f32::from(shade.color[channel]));
                // A share from 0 to 1 keeps the blend between two channel values.
                (low + (high - low) * share).round() as u8
            }));
        }
    }
    colors
}

/// Returns the largest of the shades of `rule` at the cell of the colour pass, and the palette entry its bands start
/// at after the `states` entries of the states, written at `site`.
///
/// # Errors
///
/// Returns the message for a rule with no shades, or a shade of no Number property of the ground.
fn largest_shade(
    builder: &mut Builder<'_>,
    site: &Site,
    rule: &crate::sheet::document::ColorRule,
    states: usize,
) -> Result<(InputValue, InputValue), String> {
    // The largest shade, and the first entry of its bands.
    let mut largest: Option<(InputValue, InputValue)> = None;
    for (index, shade) in rule.shades.iter().enumerate() {
        let value = builder.read(site.pass, &shade.property)?;
        let value = builder.input_as(site, &value, &ValueType::F32)?;
        let offset = InputValue::Literal(Literal::U32(states as u32 + index as u32 * SHADE_BANDS));
        largest = Some(match largest {
            None => (value, offset),
            Some((best, best_offset)) => {
                let above = builder.node_input(site, "cmp.gt", vec![("a", value.clone()), ("b", best.clone())]);
                let best = builder.select(site, above.clone(), value, best);
                let offset = builder.select(site, above, offset, best_offset);
                (best, offset)
            }
        });
    }
    largest.ok_or_else(|| "the colour rule needs both a shaded state and its shades, or neither".to_owned())
}

/// Returns the band from 0 to [`SHADE_BANDS`] that the value `best` falls in, written at `site`: 0 for a value at or
/// below 0, and on a log scale over `decades` decades below 1 for a value below the lowest decade.
fn shade_band(builder: &mut Builder<'_>, site: &Site, best: InputValue, log: bool, decades: Option<u32>) -> InputValue {
    let share = if log {
        let decades = decades.unwrap_or(3).max(1);
        expression(
            builder,
            site,
            &format!(
                "log10(max(v, {})) / {} + 1.0",
                expression_number(f64::from(SMALLEST_SHADE)),
                expression_number(f64::from(decades))
            ),
            vec![("v", best.clone())],
        )
    } else {
        best.clone()
    };
    let none = expression(builder, site, "min(v, s)", vec![("v", best), ("s", share.clone())]);
    let none = builder.node_input(site, "cmp.le", vec![("a", none), ("b", number(0.0))]);
    let scaled = builder.node_input(site, "math.mul", vec![("a", share), ("b", number(SHADE_BANDS as f32))]);
    let whole = builder.node_input(site, "conv.to_u32", vec![("a", scaled)]);
    let clamped = builder.node_input(
        site,
        "math.clamp",
        vec![
            ("x", whole),
            ("lo", InputValue::Literal(Literal::U32(1))),
            ("hi", InputValue::Literal(Literal::U32(SHADE_BANDS))),
        ],
    );
    builder.select(site, none, InputValue::Literal(Literal::U32(0)), clamped)
}

/// Returns the palette entry of a shaded cell, written at `site`: `offset + band - 1` for a band above 0, and the
/// shade state's own entry `own` otherwise.
fn shade_entry(builder: &mut Builder<'_>, site: &Site, band: InputValue, offset: InputValue, own: u32) -> InputValue {
    let some = builder.node_input(
        site,
        "cmp.gt",
        vec![("a", band.clone()), ("b", InputValue::Literal(Literal::U32(0)))],
    );
    let entry = builder.node_input(site, "math.add", vec![("a", offset), ("b", band)]);
    let entry = builder.node_input(
        site,
        "math.sub",
        vec![("a", entry), ("b", InputValue::Literal(Literal::U32(1)))],
    );
    builder.select(site, some, entry, InputValue::Literal(Literal::U32(own)))
}
