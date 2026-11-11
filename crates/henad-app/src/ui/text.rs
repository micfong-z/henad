//! Text helpers the tabs share: error messages, capitals, durations, folder names and the width of a line of text.

use std::error::Error;
use std::path::Path;
use std::time::Duration;

/// Returns `error` and each of its causes, joined by colons, starting with a capital letter.
pub fn describe_error(error: &dyn Error) -> String {
    capitalize(&error_chain(error))
}

/// Returns `error` and each of its causes, joined by colons.
pub fn error_chain(error: &dyn Error) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(source) = cause {
        text.push_str(": ");
        text.push_str(&source.to_string());
        cause = source.source();
    }
    text
}

/// Returns `text` with its first letter in upper case.
pub fn capitalize(text: &str) -> String {
    let mut characters = text.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().chain(characters).collect())
        .unwrap_or_default()
}

/// Returns `duration` rounded down for a progress line.
///
/// It returns "under 1 ms" below a millisecond, milliseconds under a second, seconds under a minute, minutes and
/// seconds under ten minutes, minutes under an hour, and hours and minutes past that.
pub fn format_duration(duration: Duration) -> String {
    let seconds = duration.as_secs();
    let (hours, minutes) = (seconds / 3600, seconds % 3600 / 60);
    match seconds {
        0 if duration.subsec_millis() == 0 => "under 1 ms".to_owned(),
        0 => format!("{} ms", duration.subsec_millis()),
        1..60 => format!("{seconds} s"),
        60..600 => format!("{minutes} min {} s", seconds % 60),
        600..3600 => format!("{minutes} min"),
        _ => format!("{hours} h {minutes} min"),
    }
}

/// Returns the last part of `folder`'s path, or the whole path when it has no file name.
pub fn folder_name(folder: &Path) -> String {
    folder.file_name().map_or_else(
        || folder.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Returns the width of `text` in `style` on one line.
///
/// Note that a text style the `Ui` overrides takes the place of `style`.
pub fn text_width(ui: &egui::Ui, text: &str, style: egui::TextStyle) -> f32 {
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style);
    galley.size().x
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::format_duration;

    #[test]
    fn a_duration_is_rounded_to_the_units_a_progress_line_needs() {
        assert_eq!(format_duration(Duration::ZERO), "under 1 ms");
        assert_eq!(format_duration(Duration::from_micros(900)), "under 1 ms");
        assert_eq!(format_duration(Duration::from_micros(40_700)), "40 ms");
        assert_eq!(format_duration(Duration::from_millis(480)), "480 ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999 ms");
        assert_eq!(format_duration(Duration::from_millis(1_000)), "1 s");
        assert_eq!(format_duration(Duration::from_millis(45_900)), "45 s");
        assert_eq!(format_duration(Duration::from_secs(250)), "4 min 10 s");
        assert_eq!(format_duration(Duration::from_secs(1_530)), "25 min");
        assert_eq!(format_duration(Duration::from_secs(4_830)), "1 h 20 min");
    }
}
