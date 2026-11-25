//! Default colours of new states and measures, the palette a state's colour is chosen from, and the usual colours of
//! common state names.

use crate::value::Color;

// Colours of the defaults, by name.
const DARK_GRAY: Color = [0x42, 0x42, 0x42, 0xFF];
const BLUE: Color = [0x00, 0x7A, 0xF5, 0xFF];
const ORANGE: Color = [0xEC, 0x6F, 0x27, 0xFF];
const PURPLE: Color = [0x91, 0x54, 0xFF, 0xFF];
const YELLOW: Color = [0xFF, 0xC1, 0x07, 0xFF];
const GRAY: Color = [0x80, 0x80, 0x80, 0xFF];
const DARK_GREEN: Color = [0x08, 0x68, 0x2F, 0xFF];
const BROWN: Color = [0x62, 0x43, 0x09, 0xFF];
const WHITE: Color = [0xF5, 0xF5, 0xF5, 0xFF];
const GREEN: Color = [0x14, 0xAE, 0x52, 0xFF];
const RED: Color = [0xE4, 0x37, 0x48, 0xFF];
const BLACK: Color = [0x13, 0x13, 0x13, 0xFF];

/// Colours new states take in order, so the defaults carry no meaning: dark grey, blue, orange, purple, yellow, grey,
/// dark green, brown, white, green, red and black.
pub const STATE_COLORS: [Color; 12] = [
    DARK_GRAY, BLUE, ORANGE, PURPLE, YELLOW, GRAY, DARK_GREEN, BROWN, WHITE, GREEN, RED, BLACK,
];

/// Colours of the palette a state's colour is chosen from, in the order of [`crate::sheet::sentence::COLOR_NAMES`].
///
/// They are the colours of [`STATE_COLORS`] in another order: black, dark grey, grey, white, red, orange, yellow,
/// brown, green, dark green, blue and purple.
pub const PALETTE_COLORS: [Color; 12] = [
    BLACK, DARK_GRAY, GRAY, WHITE, RED, ORANGE, YELLOW, BROWN, GREEN, DARK_GREEN, BLUE, PURPLE,
];

/// Colours a measure that counts no state takes in order: blue, red, green, purple, orange, yellow and grey.
pub const MEASURE_COLORS: [Color; 7] = [BLUE, RED, GREEN, PURPLE, ORANGE, YELLOW, GRAY];

/// Returns the usual colour of a state named `name`, whole name and case aside, such as red for "Infected", or `None`
/// for a name with no usual colour.
pub fn suggested_state_color(name: &str) -> Option<Color> {
    let name = name.trim().to_lowercase();
    let usual: [(&[&str], Color); 6] = [
        (&["infected", "infectious", "sick", "ill"], RED),
        (&["burning", "fire", "on fire"], ORANGE),
        (
            &["tree", "trees", "forest", "grass", "plant", "plants", "vegetation"],
            GREEN,
        ),
        (&["recovered", "removed", "immune"], GRAY),
        (&["susceptible", "healthy"], BLUE),
        (&["dead"], BLACK),
    ];
    usual
        .iter()
        .find(|(names, _)| names.contains(&name.as_str()))
        .map(|(_, color)| *color)
}
