//! The Seed row of the Parameters panel: the field, its dice button, and the seed read from its text.

use std::num::ParseIntError;

use crate::icons::material_design_icons::{MDI_DICE_5, MDI_RESTART};
use crate::state::AppState;
use crate::ui::sweep::layout::add_button;
use henad_core::authoring::primitives::rng::mix_seed;
use web_time::{SystemTime, UNIX_EPOCH};

/// Line shown under the Seed field while its text is not a seed.
pub const INVALID_SEED: &str = "Seed must be an integer from 0 to 18446744073709551615.";

/// Space between the Seed field's frame and its text, egui's default for a text field.
const SEED_FIELD_MARGIN: egui::Margin = egui::Margin::symmetric(4, 2);

/// Draws the Seed field and its dice button, with an error line while [`parse_seed`] rejects the field's text.
pub(super) fn seed_row(ui: &mut egui::Ui, app: &mut AppState) {
    let pending = app.seed_pending();
    let hint = if pending {
        format!("Seed changed. Press {MDI_RESTART}\u{a0}Build to apply.")
    } else {
        "Seed for random number generation.".to_owned()
    };
    let mut label = egui::RichText::new(format!("Seed {MDI_RESTART}"));
    if pending {
        label = label.color(ui.visuals().warn_fg_color);
    }
    let width = seed_field_width(ui);

    ui.horizontal(|ui| {
        let field = egui::TextEdit::singleline(&mut app.seed_text)
            .hint_text("Default")
            .margin(SEED_FIELD_MARGIN)
            .desired_width(width);
        let field = ui.add(field).on_hover_text(hint.as_str());
        if field.changed()
            && let Ok(seed) = parse_seed(&app.seed_text)
        {
            app.seed = seed;
        }
        if dice_button(ui).clicked() {
            app.set_seed(Some(draw_seed(app.seed)));
        }
        let label = ui.label(label).on_hover_text(hint.as_str());
        field.labelled_by(label.id);
    });

    if parse_seed(&app.seed_text).is_err() {
        ui.colored_label(ui.visuals().error_fg_color, INVALID_SEED);
    }
}

/// Adds the button beside the Seed field that draws a random seed.
pub(super) fn dice_button(ui: &mut egui::Ui) -> egui::Response {
    add_button(ui, true, egui::Button::new(MDI_DICE_5), "Generate random seed").on_hover_text("Generate random seed")
}

/// Returns the width of a Seed field that shows every digit of the largest seed.
pub(crate) fn seed_field_width(ui: &egui::Ui) -> f32 {
    let font = egui::FontSelection::Default.resolve(ui.style());
    let digits = ui.fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(u64::MAX.to_string(), font, egui::Color32::PLACEHOLDER)
            .size()
            .x
    });
    digits + SEED_FIELD_MARGIN.sum().x + ui.visuals().text_cursor.stroke.width
}

/// Returns the seed in the Seed field's `text`, `None` for an empty field.
///
/// `None` stands for the model's default seed. No number equals it.
///
/// # Errors
///
/// Returns the parse error for text that is not a whole number from 0 to `u64::MAX`.
pub fn parse_seed(text: &str) -> Result<Option<u64>, ParseIntError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    text.parse().map(Some)
}

/// Returns a seed drawn from the clock, mixed with `previous` so two draws in one clock tick differ.
pub(crate) fn draw_seed(previous: Option<u64>) -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos() as u64);
    mix_seed(nanos ^ previous.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::{INVALID_SEED, parse_seed};

    #[test]
    fn an_empty_seed_field_is_the_default_and_anything_else_a_whole_number() {
        assert_eq!(parse_seed(""), Ok(None), "empty");
        assert_eq!(parse_seed("  "), Ok(None), "blank");
        assert_eq!(parse_seed(" 42 "), Ok(Some(42)), "padded");
        assert_eq!(parse_seed("0"), Ok(Some(0)), "zero parses as seed 0");
        assert_eq!(parse_seed("18446744073709551615"), Ok(Some(u64::MAX)), "max u64");
        assert!(parse_seed("-1").is_err(), "negative");
        assert!(parse_seed("abc").is_err(), "text");
        assert!(parse_seed("1.5").is_err(), "fraction");
        assert!(parse_seed("18446744073709551616").is_err(), "overflow");
        assert!(
            INVALID_SEED.contains(&u64::MAX.to_string()),
            "the error names the largest seed"
        );
    }
}
