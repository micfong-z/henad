//! Colours of the cells: none when the cells take the colours of their first States property, and otherwise the view
//! pass `colours`. The engine draws the first States property without a view pass.
//!
//! A cell coloured by the states of another property reads that property through `view.color` and its category. A
//! cell shaded by a number takes a band of the palette `shaded_<property>` over 0 to 1 through `view.ramp`. Only a
//! linear shading lowers, so a rule with a log scale or a number of decades is refused.

use super::ids::{GROUP_PART, Owner, SheetKey};
use super::{Builder, Site, text};
use crate::document::{Domain, Palette, Pass, PassStage};
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

/// Writes the view pass of the cells' colour rule, when the rule needs one.
pub(super) fn lower(builder: &mut Builder<'_>) {
    let Some(rule) = builder.document.colors.get("cells") else {
        return;
    };
    let place = Place::Color("cells".to_owned());
    if rule.scale == Some(ShadeScale::Log) || rule.decades.is_some() {
        builder.problem(
            "the colour rule of the cells shades on a log scale, and a grid's shading lowers on a linear scale only",
            place,
        );
        return;
    }
    let Some((index, property)) = rule.property.as_deref().and_then(|id| builder.property(id)) else {
        builder.problem("the colour rule of the cells names no property of the cells", place);
        return;
    };
    let first_states = builder
        .cells()
        .iter()
        .position(|candidate| candidate.property_type == PropertyType::States);
    let shaded = match rule.rule {
        ColorRuleKind::ByState if rule.shade_state.is_some() || !rule.shades.is_empty() => {
            builder.problem(
                "the colour rule of the cells shades a state, and only the ground of agents shades a state",
                place,
            );
            return;
        }
        ColorRuleKind::ByState if first_states == Some(index) => return,
        ColorRuleKind::ByState => false,
        ColorRuleKind::Shaded if property.property_type != PropertyType::States => true,
        ColorRuleKind::Shaded | ColorRuleKind::ByHeading => {
            builder.problem(
                "the colour rule of the cells cannot colour the cells by this property",
                place,
            );
            return;
        }
    };
    let pass = builder.add_pass(Pass::new(COLOR_PASS, PassStage::View, Some(Domain::Cells)));
    let site = Site {
        pass,
        owner: Owner::Sheet(SheetKey {
            group: GROUP_PART,
            index: 0,
        }),
        path: "color.cells".to_owned(),
        region: None,
    };
    let read = match builder.read(pass, &property.id) {
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

/// Writes the shading of the number `read`, the read of property `property`, at `site`.
///
/// # Errors
///
/// Returns the message for a palette id that a property's category takes, or a value with no number.
fn shade(builder: &mut Builder<'_>, site: &Site, read: &super::value::Operand, property: &str) -> Result<(), String> {
    let palette = format!("shaded_{property}");
    if builder.graph.categories.iter().any(|category| category.id == palette) {
        return Err(format!(
            "the palette of the shaded cells would take the id '{palette}' of a property"
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
