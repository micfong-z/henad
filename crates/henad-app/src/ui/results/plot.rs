//! Drawing helpers the result views share: colours, decimation, level labels, a colour bar, the labeled combo boxes
//! of the views' controls, and the pace at which a view computes again while runs arrive.

use std::ops::RangeInclusive;
use std::time::Duration;

use egui::Color32;
use egui_plot::GridMark;
use web_time::Instant;

/// Colours of the configurations a view draws, in order. A config's band and mean line share one.
pub const CONFIG_COLORS: [Color32; 10] = [
    Color32::from_rgb(0x4e, 0x79, 0xa7),
    Color32::from_rgb(0xf2, 0x8e, 0x2b),
    Color32::from_rgb(0xe1, 0x57, 0x59),
    Color32::from_rgb(0x76, 0xb7, 0xb2),
    Color32::from_rgb(0x59, 0xa1, 0x4f),
    Color32::from_rgb(0xed, 0xc9, 0x48),
    Color32::from_rgb(0xb0, 0x7a, 0xa1),
    Color32::from_rgb(0xff, 0x9d, 0xa7),
    Color32::from_rgb(0x9c, 0x75, 0x5f),
    Color32::from_rgb(0xba, 0xb0, 0xac),
];

/// Stops of the heatmap scale, low to high, sampled from viridis.
const HEAT_STOPS: [Color32; 5] = [
    Color32::from_rgb(0x44, 0x01, 0x54),
    Color32::from_rgb(0x3b, 0x52, 0x8b),
    Color32::from_rgb(0x21, 0x91, 0x8c),
    Color32::from_rgb(0x5e, 0xc9, 0x62),
    Color32::from_rgb(0xfd, 0xe7, 0x25),
];

/// Colour of a heatmap cell that holds no value.
pub const NO_DATA_COLOR: Color32 = Color32::from_gray(110);

/// Most points a line keeps once decimated.
pub const MAX_PLOT_POINTS: usize = 1000;

/// Shortest time between two computations of a view while runs keep arriving.
pub const REFRESH_INTERVAL: Duration = Duration::from_millis(500);

/// Returns whether a view computed at `computed_at`, from a store that has changed since, is due to compute again.
///
/// Before [`REFRESH_INTERVAL`] has passed, `ctx` repaints once it has.
pub fn refresh_due(ctx: &egui::Context, computed_at: Instant) -> bool {
    match REFRESH_INTERVAL.checked_sub(computed_at.elapsed()) {
        Some(wait) if !wait.is_zero() => {
            ctx.request_repaint_after(wait);
            false
        }
        _ => true,
    }
}

/// Returns the colour of config number `position` in a view.
pub fn config_color(position: usize) -> Color32 {
    CONFIG_COLORS[position % CONFIG_COLORS.len()]
}

/// Returns the colour of `value` on the heatmap scale from `min` to `max`, or [`NO_DATA_COLOR`] for a value that is
/// not finite.
///
/// A scale whose ends meet maps every finite value to its middle.
pub fn heat_color(value: f64, min: f64, max: f64) -> Color32 {
    if !value.is_finite() {
        return NO_DATA_COLOR;
    }
    let fraction = if max > min {
        ((value - min) / (max - min)).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let scaled = fraction * (HEAT_STOPS.len() - 1) as f64;
    let lower = (scaled.floor() as usize).min(HEAT_STOPS.len() - 2);
    let blend = (scaled - lower as f64) as f32;
    blend_color(HEAT_STOPS[lower], HEAT_STOPS[lower + 1], blend)
}

/// Returns the colour `blend` of the way from `low` to `high`.
fn blend_color(low: Color32, high: Color32, blend: f32) -> Color32 {
    let channel = |low_channel: u8, high_channel: u8| {
        let (low_channel, high_channel) = (f32::from(low_channel), f32::from(high_channel));
        (low_channel + (high_channel - low_channel) * blend).round() as u8
    };
    Color32::from_rgb(
        channel(low.r(), high.r()),
        channel(low.g(), high.g()),
        channel(low.b(), high.b()),
    )
}

/// Returns `points` thinned to at most `max_points`, keeping the first and last point and the lowest and highest
/// point of every bucket in between.
///
/// Points keep their order, so a spike survives however far a long series is thinned. Note that a `max_points` below
/// 4 leaves `points` as they are.
pub fn decimate(points: &[[f64; 2]], max_points: usize) -> Vec<[f64; 2]> {
    let [first, interior @ .., last] = points else {
        return points.to_vec();
    };
    if points.len() <= max_points || max_points < 4 {
        return points.to_vec();
    }
    let buckets = (max_points - 2) / 2;
    let bucket_size = interior.len().div_ceil(buckets);
    let mut kept = Vec::with_capacity(max_points);
    kept.push(*first);
    for bucket in interior.chunks(bucket_size) {
        let lowest = extreme_position(bucket, |a, b| a < b);
        let highest = extreme_position(bucket, |a, b| a > b);
        let (earlier, later) = (lowest.min(highest), lowest.max(highest));
        kept.push(bucket[earlier]);
        if later != earlier {
            kept.push(bucket[later]);
        }
    }
    kept.push(*last);
    kept
}

/// Returns the position in `bucket` of the point whose value `beats` every other, the first on a tie. A value that is
/// not finite never wins over one that is.
fn extreme_position(bucket: &[[f64; 2]], beats: impl Fn(f64, f64) -> bool) -> usize {
    let mut best = 0;
    for (position, point) in bucket.iter().enumerate().skip(1) {
        let current = bucket[best][1];
        if point[1].is_finite() && (!current.is_finite() || beats(point[1], current)) {
            best = position;
        }
    }
    best
}

/// Returns an axis formatter that names the mark at index `i` by `levels[i]`, and leaves every other mark blank.
pub fn level_formatter(levels: &[String]) -> impl Fn(GridMark, &RangeInclusive<f64>) -> String + '_ {
    move |mark, _| {
        let index = mark.value.round();
        if (mark.value - index).abs() > 1e-6 || index < 0.0 {
            return String::new();
        }
        levels.get(index as usize).cloned().unwrap_or_default()
    }
}

/// Draws a horizontal bar of the heatmap scale from `min` to `max`, labelled at both ends.
pub fn color_bar(ui: &mut egui::Ui, min: f64, max: f64) {
    const STEPS: usize = 32;
    ui.horizontal(|ui| {
        ui.label(format_significant(min));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 12.0), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let step_width = rect.width() / STEPS as f32;
        for step in 0..STEPS {
            let fraction = (step as f64 + 0.5) / STEPS as f64;
            let left = rect.left() + step as f32 * step_width;
            let cell = egui::Rect::from_min_size(
                egui::pos2(left, rect.top()),
                egui::vec2(step_width + 0.5, rect.height()),
            );
            painter.rect_filled(cell, 0.0, heat_color(fraction, 0.0, 1.0));
        }
        ui.label(format_significant(max));
    });
}

/// Draws `label` and a combo box on one row of a wrapping layout, and returns the combo box's response, labeled by
/// `label`.
///
/// The row wraps before the two when they do not fit the rest of it. A combo box's width is known only once it is
/// placed, so the layout cannot wrap it on its own.
pub fn labeled_combo(
    ui: &mut egui::Ui,
    label: &str,
    id_salt: impl egui::AsIdSalt,
    selected_text: &str,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let spacing = ui.spacing();
    let button_extra = spacing.icon_spacing + spacing.icon_width + 2.0 * spacing.button_padding.x;
    let (combo_width, item_spacing) = (spacing.combo_width, spacing.item_spacing.x);
    let selected_width = (text_width(ui, selected_text, egui::TextStyle::Button) + button_extra).max(combo_width);
    let pair_width = text_width(ui, label, egui::TextStyle::Body) + item_spacing + selected_width;
    let row_started = ui.cursor().left() > ui.max_rect().left() + 0.5;
    if row_started && ui.available_size_before_wrap().x < pair_width {
        ui.end_row();
    }
    let label = ui.add(egui::Label::new(label).wrap_mode(egui::TextWrapMode::Extend));
    egui::ComboBox::from_id_salt(id_salt)
        .selected_text(selected_text)
        .truncate()
        .show_ui(ui, add_contents)
        .response
        .labelled_by(label.id)
}

/// Returns the width of `text` in `style` on one line.
pub fn text_width(ui: &egui::Ui, text: &str, style: egui::TextStyle) -> f32 {
    let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Extend), f32::INFINITY, style);
    galley.size().x
}

/// Returns `value` with four significant digits, or a dash for a value that is not finite.
///
/// A value from 1000 up to a trillion is rounded to a whole number with its digits grouped in threes. Only a value
/// past that range, or below a thousandth, takes an exponent.
pub fn format_significant(value: f64) -> String {
    if !value.is_finite() {
        return "–".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let magnitude = value.abs().log10().floor();
    if !(-3.0..12.0).contains(&magnitude) {
        return format!("{value:.3e}");
    }
    if magnitude >= 3.0 {
        return group_digits(value.round());
    }
    let decimals = (3.0 - magnitude) as usize;
    let text = format!("{value:.decimals$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// Returns `value` with six significant digits and no trailing zeros, or a dash for a value that is not finite.
///
/// A value from a million up keeps every digit of its whole part. The whole part of a value from 1000 up has its
/// digits grouped in threes, as in `3,108.5`. Only a value from a trillion up, or below a thousandth, takes an
/// exponent.
pub fn format_precise(value: f64) -> String {
    if !value.is_finite() {
        return "–".to_owned();
    }
    if value == 0.0 {
        return "0".to_owned();
    }
    let magnitude = value.abs().log10().floor();
    if !(-3.0..12.0).contains(&magnitude) {
        return format!("{value:.5e}");
    }
    let decimals = (5.0 - magnitude).max(0.0) as usize;
    let text = format!("{:.decimals$}", value.abs());
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        text.as_str()
    };
    let (whole, fraction) = text
        .split_once('.')
        .map_or((text, None), |(whole, fraction)| (whole, Some(fraction)));
    let mut formatted = String::new();
    if value < 0.0 {
        formatted.push('-');
    }
    formatted.push_str(&group_whole(whole));
    if let Some(fraction) = fraction {
        formatted.push('.');
        formatted.push_str(fraction);
    }
    formatted
}

/// Returns the whole number `value` with its digits grouped in threes by commas.
fn group_digits(value: f64) -> String {
    let mut grouped = String::new();
    if value < 0.0 {
        grouped.push('-');
    }
    grouped.push_str(&group_whole(&format!("{:.0}", value.abs())));
    grouped
}

/// Returns the digits `digits` grouped in threes by commas.
fn group_whole(digits: &str) -> String {
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (position, digit) in digits.chars().enumerate() {
        if position > 0 && (digits.len() - position).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

#[cfg(test)]
mod tests {
    use super::{NO_DATA_COLOR, decimate, format_precise, format_significant, heat_color};

    fn series(values: &[f64]) -> Vec<[f64; 2]> {
        values.iter().enumerate().map(|(x, &y)| [x as f64, y]).collect()
    }

    #[test]
    fn decimation_keeps_the_extremes_and_the_ends() {
        let mut values: Vec<f64> = (0..10_000).map(|x| (f64::from(x) * 0.01).sin()).collect();
        values[1234] = 50.0;
        values[7777] = -40.0;
        let points = series(&values);
        let kept = decimate(&points, 100);

        assert!(kept.len() <= 100, "{} points kept", kept.len());
        assert_eq!(kept.first(), points.first());
        assert_eq!(kept.last(), points.last());
        assert!(kept.contains(&[1234.0, 50.0]), "the spike is dropped");
        assert!(kept.contains(&[7777.0, -40.0]), "the dip is dropped");
        assert!(
            kept.windows(2).all(|pair| pair[0][0] < pair[1][0]),
            "points keep their order"
        );
    }

    #[test]
    fn a_short_series_is_left_whole() {
        let points = series(&[1.0, 3.0, 2.0]);
        assert_eq!(decimate(&points, 100), points);
    }

    #[test]
    fn a_value_that_is_not_finite_never_hides_an_extreme() {
        let mut values = vec![0.0; 1000];
        values[10] = f64::NAN;
        values[11] = 9.0;
        let kept = decimate(&series(&values), 10);
        assert!(kept.contains(&[11.0, 9.0]));
    }

    #[test]
    fn the_heat_scale_marks_missing_values() {
        assert_eq!(heat_color(f64::NAN, 0.0, 1.0), NO_DATA_COLOR);
        assert_ne!(heat_color(0.0, 0.0, 1.0), heat_color(1.0, 0.0, 1.0));
        assert_eq!(
            heat_color(-5.0, 0.0, 1.0),
            heat_color(0.0, 0.0, 1.0),
            "a low value clamps to the scale"
        );
        assert_eq!(heat_color(3.0, 3.0, 3.0), heat_color(0.5, 0.0, 1.0));
    }

    #[test]
    fn values_are_written_with_four_significant_digits() {
        assert_eq!(format_significant(0.123_456), "0.1235");
        assert_eq!(format_significant(123.456), "123.5");
        assert_eq!(format_significant(2.0), "2");
        assert_eq!(format_significant(f64::NAN), "–");
        assert_eq!(format_significant(0.000_123_4), "1.234e-4");
    }

    #[test]
    fn large_values_are_whole_numbers_with_grouped_digits() {
        assert_eq!(format_significant(1234.56), "1,235");
        assert_eq!(format_significant(1_038_345.0), "1,038,345");
        assert_eq!(format_significant(-15_000_000.4), "-15,000,000");
        assert_eq!(format_significant(999_999.6), "1,000,000");
        assert_eq!(format_significant(2.5e13), "2.500e13");
    }

    #[test]
    fn a_precise_value_keeps_six_significant_digits() {
        assert_eq!(format_precise(3108.5), "3,108.5");
        assert_eq!(format_precise(3109.0), "3,109");
        assert_eq!(format_precise(102.333_333), "102.333");
        assert_eq!(format_precise(-0.123_456_78), "-0.123457");
        assert_eq!(format_precise(1_234_567.8), "1,234,568");
        assert_eq!(format_precise(0.0), "0");
        assert_eq!(format_precise(f64::NEG_INFINITY), "–");
        assert_eq!(format_precise(2.5e13), "2.50000e13");
    }
}
