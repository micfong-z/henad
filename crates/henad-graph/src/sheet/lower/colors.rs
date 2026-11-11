//! Colours of the cells or agents: none when they take the colours of their first States property, and otherwise the
//! view pass `colours`. The engine draws the first States property without a view pass. The ground's colours lower as
//! [`super::shades`] describes.
//!
//! An agent coloured by its heading takes one of the eight compass colours of the palette `by_heading` through
//! `space.heading_octant`.
//! A cell coloured by the states of another property reads that property through `view.color` and its category. A
//! cell shaded by a number takes a band of the palette `shaded_<property>` through `view.ramp`, from the rule's
//! darkest value (0 when left out) to its lightest value (1 when left out). The palette runs from the dark end of the
//! rule's hue to its light end, or through a blue to orange scale for a rule with no hue. A rule darkest at the higher
//! value reverses the palette and passes the lower value to `view.ramp` as its low end. Only a linear shading lowers,
//! so a rule with a log scale or a number of decades is refused.

use super::ids::GROUP_PART;
use super::value::{number, output};
use super::{Builder, Site, text};
use crate::document::{Palette, Pass, PassStage};
use crate::sheet::check::Place;
use crate::sheet::document::{ColorRule, ColorRuleKind, PropertyType, ShadeHue, ShadeScale};
use crate::types::ValueType;

/// Id of the view pass of the cells' colours.
const COLOR_PASS: &str = "colours";

/// Colours of the palette of a shaded number with no hue, from its low end to its high end.
const SHADE_COLORS: [[u8; 4]; 8] = [
    [0, 92, 230, 255],
    [40, 120, 235, 255],
    [90, 150, 240, 255],
    [150, 180, 240, 255],
    [245, 190, 130, 255],
    [245, 160, 80, 255],
    [240, 130, 40, 255],
    [230, 100, 0, 255],
];

/// Palette id and colours of an agent coloured by its heading, from east anticlockwise.
const HEADING_PALETTE: (&str, [[u8; 4]; 8]) = (
    "by_heading",
    [
        [228, 55, 72, 255],
        [240, 122, 40, 255],
        [255, 193, 7, 255],
        [139, 195, 74, 255],
        [30, 168, 124, 255],
        [0, 158, 200, 255],
        [0, 122, 245, 255],
        [142, 84, 216, 255],
    ],
);

/// Writes the view pass of the colour rule of the sheet's kind of thing, when the rule needs one.
pub(super) fn lower(builder: &mut Builder<'_>) {
    if builder.is_ground() {
        super::shades::colors(builder);
        return;
    }
    let document = builder.document;
    let thing = builder.thing_id();
    let Some(rule) = document.colors.get(thing) else {
        return;
    };
    if let Err(message) = color_rule(builder, rule) {
        builder.problem(message, Place::Color(thing.to_owned()));
    }
}

/// Writes the view pass of the colour rule `rule` of the sheet's kind of thing, when the rule needs one.
///
/// # Errors
///
/// Returns the message for a rule this version does not lower, or that names no property it can colour by.
fn color_rule(builder: &mut Builder<'_>, rule: &ColorRule) -> Result<(), String> {
    if rule.scale == Some(ShadeScale::Log) || rule.decades.is_some() {
        return Err(format!(
            "the colour rule of {} shades on a log scale, and shading lowers on a linear scale only",
            builder.thing_words()
        ));
    }
    if rule.rule == ColorRuleKind::ByHeading && builder.is_agents() {
        return by_heading(builder);
    }
    let (index, property) = rule
        .property
        .as_deref()
        .and_then(|id| builder.property(id))
        .ok_or_else(|| format!("the colour rule names no property of {}", builder.thing_words()))?;
    let first_states = builder
        .properties()
        .iter()
        .position(|candidate| candidate.property_type == PropertyType::States);
    let shaded = match rule.rule {
        ColorRuleKind::ByState if rule.shade_state.is_some() || !rule.shades.is_empty() => {
            return Err(format!(
                "the colour rule of {} shades a state, and only the ground of agents shades a state",
                builder.thing_words()
            ));
        }
        ColorRuleKind::ByState if first_states == Some(index) => return Ok(()),
        ColorRuleKind::ByState => false,
        ColorRuleKind::Shaded if property.property_type != PropertyType::States => true,
        ColorRuleKind::Shaded | ColorRuleKind::ByHeading => {
            return Err(format!(
                "the colour rule cannot colour {} by this property",
                builder.thing_words()
            ));
        }
    };
    let site = color_site(builder);
    let read = builder.read(site.pass, &property.id)?;
    if shaded {
        return shade(builder, &site, &read, &property.id, rule);
    }
    let index_input = builder.input_as(&site, &read, &builder.lane_type(property))?;
    builder.emit(
        &site,
        "view.color",
        vec![("palette", text(&property.id))],
        vec![("index".to_owned(), index_input)],
    );
    Ok(())
}

/// Adds the view pass of the colours, `colours` or `colours_ground` for the ground, and returns the site of its nodes.
pub(super) fn color_site(builder: &mut Builder<'_>) -> Site {
    let id = if builder.is_ground() {
        format!("{COLOR_PASS}_ground")
    } else {
        COLOR_PASS.to_owned()
    };
    let pass = builder.add_pass(Pass::new(&id, PassStage::View, Some(builder.domain())));
    Site::sheet(pass, GROUP_PART, 0, format!("color.{}", builder.thing_id()))
}

/// Writes the colour of each agent by its heading: the palette of eight compass colours, and the octant of the heading
/// through it.
///
/// # Errors
///
/// Returns the message for a palette id that a property's category takes, or agents with no Heading.
fn by_heading(builder: &mut Builder<'_>) -> Result<(), String> {
    let (palette, colors) = HEADING_PALETTE;
    if builder.graph.categories.iter().any(|category| category.id == palette) {
        return Err(format!(
            "the palette of the headings would take the id '{palette}' of a property"
        ));
    }
    let heading = builder
        .properties()
        .iter()
        .find(|property| property.property_type == PropertyType::Heading)
        .ok_or_else(|| "the agents have no Heading to colour them by".to_owned())?;
    builder.graph.palettes.push(Palette::new(palette, colors.to_vec()));
    let site = color_site(builder);
    let read = builder.read(site.pass, &heading.id)?;
    let read = builder.input_as(&site, &read, &ValueType::Vec2F)?;
    let octant = builder.emit(&site, "space.heading_octant", Vec::new(), vec![("v".to_owned(), read)]);
    builder.emit(
        &site,
        "view.color",
        vec![("palette", text(palette))],
        vec![("index".to_owned(), output(octant))],
    );
    Ok(())
}

/// Writes the shading of the number `read`, the read of property `property`, at `site`, in the hue and over the range
/// of `rule`.
///
/// # Errors
///
/// Returns the message for a palette id that a property's category takes, a range whose two ends are equal, or a value
/// with no number.
pub(super) fn shade(
    builder: &mut Builder<'_>,
    site: &Site,
    read: &super::value::Operand,
    property: &str,
    rule: &ColorRule,
) -> Result<(), String> {
    let palette = format!("shaded_{property}");
    if builder.graph.categories.iter().any(|category| category.id == palette) {
        return Err(format!(
            "the palette of the shading would take the id '{palette}' of a property"
        ));
    }
    let darkest = rule.darkest.unwrap_or(0.0);
    let lightest = rule.lightest.unwrap_or(1.0);
    if darkest == lightest {
        return Err(format!(
            "the shading is darkest and lightest at the same value {}, and its two ends need different values",
            crate::sheet::format_number(darkest).unwrap_or_default()
        ));
    }
    let mut colors = rule.hue.map_or_else(|| SHADE_COLORS.to_vec(), hue_colors);
    // `view.ramp` takes its first band for every value when `high` is not above `low`. A rule darkest at the higher
    // value therefore ramps from `lightest` to `darkest` over the reversed palette.
    let (low, high) = if darkest > lightest {
        colors.reverse();
        (lightest, darkest)
    } else {
        (darkest, lightest)
    };
    builder.graph.palettes.push(Palette::new(&palette, colors));
    let value = builder.input_as(site, read, &ValueType::F32)?;
    let node = builder.emit(
        site,
        "view.ramp",
        vec![("palette", text(&palette))],
        vec![
            ("value".to_owned(), value),
            ("low".to_owned(), number(low as f32)),
            ("high".to_owned(), number(high as f32)),
        ],
    );
    builder.emit(
        site,
        "view.color",
        vec![("palette", text(&palette))],
        vec![("index".to_owned(), output(node))],
    );
    Ok(())
}

/// Number of colours in the palette of a shaded number.
const SHADE_STEPS: usize = 8;

/// Returns the palette of a number shaded in `hue`: [`SHADE_STEPS`] colours evenly spaced from the hue's dark end to
/// its light end.
pub(super) fn hue_colors(hue: ShadeHue) -> Vec<[u8; 4]> {
    let [dark, light] = hue.ends();
    (0..SHADE_STEPS)
        .map(|step| {
            let share = step as f32 / (SHADE_STEPS - 1) as f32;
            let mut color = [0xFF; 4];
            for (channel, (low, high)) in color.iter_mut().zip(dark.iter().zip(light)).take(3) {
                // The channels lie in 0 to 255, and so does every value between two of them.
                *channel = (f32::from(*low) + (f32::from(high) - f32::from(*low)) * share).round() as u8;
            }
            color
        })
        .collect()
}
