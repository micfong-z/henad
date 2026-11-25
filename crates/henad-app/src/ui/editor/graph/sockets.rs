//! Socket palette of the graph canvas: the colour of each value type and the pin shape of each rate.
//!
//! A socket's colour gives its type and its shape gives its rate. The colours are data colours, as the plot colours
//! of [`crate::ui::results::plot`] are. A type is no state, and MCS colours mark states.

use egui::Color32;
use egui_snarl::ui::PinShape;
use henad_graph::types::{Rate, ValueType};

// Colours of the value types.
const FLOAT: Color32 = Color32::from_rgb(0x32, 0x96, 0xfb);
const FLOAT_VECTOR: Color32 = Color32::from_rgb(0x8f, 0xc7, 0xff);
const SIGNED: Color32 = Color32::from_rgb(0x2e, 0xbf, 0x57);
const SIGNED_VECTOR: Color32 = Color32::from_rgb(0x96, 0xe2, 0x9e);
const UNSIGNED: Color32 = Color32::from_rgb(0xff, 0xcc, 0x34);
const UNSIGNED_VECTOR: Color32 = Color32::from_rgb(0xff, 0xe3, 0x8d);
const TRUTH: Color32 = Color32::from_rgb(0xbd, 0xbd, 0xbd);
const CATEGORY: Color32 = Color32::from_rgb(0xa0, 0x6c, 0xff);
const LAYER: Color32 = Color32::from_rgb(0xc2, 0x9e, 0xff);
const REFERENCE: Color32 = Color32::from_rgb(0x67, 0x36, 0xc0);

/// Colour of a socket whose type is unknown: a hole the checker could not type, or a port of broken configuration.
pub const UNKNOWN_TYPE: Color32 = Color32::from_rgb(0x80, 0x80, 0x80);

/// Returns the colour of a socket of `value_type`, or [`UNKNOWN_TYPE`] for `None`.
pub fn type_color(value_type: Option<&ValueType>) -> Color32 {
    match value_type {
        Some(ValueType::F32 | ValueType::F64) => FLOAT,
        Some(ValueType::Vec2F | ValueType::Vec2D) => FLOAT_VECTOR,
        Some(ValueType::I32) => SIGNED,
        Some(ValueType::Vec2I) => SIGNED_VECTOR,
        Some(ValueType::U32 | ValueType::U64) => UNSIGNED,
        Some(ValueType::Vec2U) => UNSIGNED_VECTOR,
        Some(ValueType::Bool) => TRUTH,
        Some(ValueType::Category(_)) => CATEGORY,
        Some(ValueType::Layer(_)) => LAYER,
        Some(ValueType::Cell | ValueType::Agent) => REFERENCE,
        Some(_) | None => UNKNOWN_TYPE,
    }
}

/// Returns the pin shape of a socket at `rate`: a circle for a uniform value, a square for a value per element, and a
/// triangle for a value per element and region iteration.
pub fn rate_shape(rate: Rate) -> PinShape {
    match rate {
        Rate::Constant | Rate::Parameter | Rate::Tick => PinShape::Circle,
        Rate::Element => PinShape::Square,
        Rate::Pair => PinShape::Triangle,
    }
}

#[cfg(test)]
mod tests {
    use egui_snarl::ui::PinShape;
    use henad_graph::types::{Rate, ValueType};

    use super::{UNKNOWN_TYPE, rate_shape, type_color};

    #[test]
    fn each_family_of_types_has_its_own_colour_and_each_rate_its_shape() {
        let families = [
            ValueType::F32,
            ValueType::Vec2F,
            ValueType::I32,
            ValueType::Vec2I,
            ValueType::U32,
            ValueType::Vec2U,
            ValueType::Bool,
            ValueType::Category("health".to_owned()),
            ValueType::Layer("pheromone".to_owned()),
            ValueType::Agent,
        ];
        let colors: Vec<_> = families.iter().map(|family| type_color(Some(family))).collect();
        for (index, color) in colors.iter().enumerate() {
            assert!(
                !colors[..index].contains(color),
                "{:?} repeats a colour",
                families[index]
            );
            assert_ne!(*color, UNKNOWN_TYPE);
        }
        assert_eq!(type_color(Some(&ValueType::F64)), type_color(Some(&ValueType::F32)));
        assert_eq!(rate_shape(Rate::Parameter), PinShape::Circle);
        assert_eq!(rate_shape(Rate::Element), PinShape::Square);
        assert_eq!(rate_shape(Rate::Pair), PinShape::Triangle);
    }
}
