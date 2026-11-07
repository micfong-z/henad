//! Colours of the cells or agents: none when they take the colours of their first States property, and otherwise the
//! view pass `colours`. The engine draws the first States property without a view pass.
//!
//! An agent coloured by its heading takes one of the eight compass colours of the palette `by_heading` through
//! `space.heading_octant`.
//! A cell coloured by the states of another property reads that property through `view.color` and its category. A
//! cell shaded by a number takes a band of the palette `shaded_<property>` over 0 to 1 through `view.ramp`. Only a
//! linear shading lowers, so a rule with a log scale or a number of decades is refused.

use super::ids::{GROUP_PART, Owner, SheetKey};
use super::{Builder, Site, text};
use crate::document::{Palette, Pass, PassStage};
use crate::sheet::check::Place;
use crate::sheet::document::{ColorRuleKind, PropertyType, ShadeScale};
use crate::types::ValueType;
use crate::value::{InputValue, Literal, PortRef};

/// Id of the view pass of the cells' colours.
const COLOR_PASS: &str = "colours";

/// Colours of the palette of a shaded number, from its low end to its high end.
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
    let thing = builder.thing_id();
    let Some(rule) = builder.document.colors.get(thing) else {
        return;
    };
    let place = Place::Color(thing.to_owned());
    if rule.scale == Some(ShadeScale::Log) || rule.decades.is_some() {
        builder.problem(
            format!(
                "the colour rule of {} shades on a log scale, and shading lowers on a linear scale only",
                builder.thing_words()
            ),
            place,
        );
        return;
    }
    if rule.rule == ColorRuleKind::ByHeading && builder.is_agents() {
        if let Err(message) = by_heading(builder) {
            builder.problem(message, place);
        }
        return;
    }
    let Some((index, property)) = rule.property.as_deref().and_then(|id| builder.property(id)) else {
        builder.problem(
            format!("the colour rule names no property of {}", builder.thing_words()),
            place,
        );
        return;
    };
    let first_states = builder
        .properties()
        .iter()
        .position(|candidate| candidate.property_type == PropertyType::States);
    let shaded = match rule.rule {
        ColorRuleKind::ByState if rule.shade_state.is_some() || !rule.shades.is_empty() => {
            builder.problem(
                format!(
                    "the colour rule of {} shades a state, and only the ground of agents shades a state",
                    builder.thing_words()
                ),
                place,
            );
            return;
        }
        ColorRuleKind::ByState if first_states == Some(index) => return,
        ColorRuleKind::ByState => false,
        ColorRuleKind::Shaded if property.property_type != PropertyType::States => true,
        ColorRuleKind::Shaded | ColorRuleKind::ByHeading => {
            builder.problem(
                format!(
                    "the colour rule cannot colour {} by this property",
                    builder.thing_words()
                ),
                place,
            );
            return;
        }
    };
    let site = color_site(builder);
    let read = match builder.read(site.pass, &property.id) {
        Ok(read) => read,
        Err(message) => {
            builder.problem(message, place);
            return;
        }
    };
    if shaded {
        if let Err(message) = shade(builder, &site, &read, &property.id) {
            builder.problem(message, place);
        }
        return;
    }
    let colored = builder
        .input_as(&site, &read, &super::world::lane_type(property), false)
        .map(|index_input| {
            builder.emit(
                &site,
                "view.color",
                vec![("palette", text(&property.id))],
                vec![("index".to_owned(), index_input)],
            );
        });
    if let Err(message) = colored {
        builder.problem(message, place);
    }
}

/// Adds the view pass of the colours and returns the site of its nodes.
fn color_site(builder: &mut Builder<'_>) -> Site {
    let pass = builder.add_pass(Pass::new(COLOR_PASS, PassStage::View, Some(builder.domain())));
    Site {
        pass,
        owner: Owner::Sheet(SheetKey {
            group: GROUP_PART,
            index: 0,
        }),
        path: format!("color.{}", builder.thing_id()),
        region: None,
    }
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
    let read = builder.input_as(&site, &read, &ValueType::Vec2F, false)?;
    let octant = builder.emit(&site, "space.heading_octant", Vec::new(), vec![("v".to_owned(), read)]);
    builder.emit(
        &site,
        "view.color",
        vec![("palette", text(palette))],
        vec![(
            "index".to_owned(),
            InputValue::Link(PortRef {
                node: octant,
                port: None,
            }),
        )],
    );
    Ok(())
}

/// Writes the shading of the number `read`, the read of property `property`, at `site`.
///
/// # Errors
///
/// Returns the message for a palette id that a property's category takes, or a value with no number.
fn shade(builder: &mut Builder<'_>, site: &Site, read: &super::value::Operand, property: &str) -> Result<(), String> {
    let palette = format!("shaded_{property}");
    if builder.graph.categories.iter().any(|category| category.id == palette) {
        return Err(format!(
            "the palette of the shading would take the id '{palette}' of a property"
        ));
    }
    builder
        .graph
        .palettes
        .push(Palette::new(&palette, SHADE_COLORS.to_vec()));
    let value = builder.input_as(site, read, &ValueType::F32, false)?;
    let node = builder.emit(
        site,
        "view.ramp",
        vec![("palette", text(&palette))],
        vec![
            ("value".to_owned(), value),
            ("low".to_owned(), InputValue::Literal(Literal::F32(0.0))),
            ("high".to_owned(), InputValue::Literal(Literal::F32(1.0))),
        ],
    );
    builder.emit(
        site,
        "view.color",
        vec![("palette", text(&palette))],
        vec![("index".to_owned(), InputValue::Link(PortRef { node, port: None }))],
    );
    Ok(())
}
